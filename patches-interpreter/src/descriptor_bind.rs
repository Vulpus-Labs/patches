//! Descriptor-level binding of a [`FlatPatch`] against the module registry.
//!
//! Produces a [`BoundPatch`] that pairs each [`FlatModule`] with the
//! [`ModuleDescriptor`] it resolves to (or marks it unresolved), and each
//! [`FlatConnection`] / [`FlatPortRef`] with the port descriptors on both
//! endpoints. No module instances are built and no
//! [`AudioEnvironment`](patches_core::AudioEnvironment) is required — this
//! is the pure descriptor-level pass ADR 0038 calls stage 3b's "partial
//! bound graph".
//!
//! Unlike [`crate::build`] this pass **never short-circuits**: every error
//! is appended to [`BoundPatch::errors`] and the walk continues, so
//! downstream consumers (LSP feature handlers under
//! accumulate-and-continue) see as much resolved information as possible
//! even when some modules fail to bind.
//!
//! Concerns covered here:
//!
//! - Module type resolution (registry lookup).
//! - Shape argument validation (delegated to `Registry::describe`).
//! - Parameter name/type/range validation (via [`crate::convert_params`]).
//! - Connection port existence on both endpoints.
//! - Cable kind and poly-layout agreement.
//! - Orphan port-ref existence against the resolved descriptor.
//!
//! Concerns that stay in [`crate::build`]:
//!
//! - Module instantiation (needs [`AudioEnvironment`](patches_core::AudioEnvironment)).
//! - Scale-range validation (graph state).
//! - Song/pattern shape checks, MasterSequencer song references.
//! - Relative file-path resolution against the patch's base dir.
//!
//! Duplicate-input detection runs here too so the LSP (which stops at
//! bind) flags `a.out -> c.in; b.out -> c.in` before the engine would.

use std::collections::HashMap;

use patches_core::{
    cables::{CableKind, PolyLayout},
    ModuleDescriptor, ParameterMap, PortDescriptor, PortRef, Provenance, QName, Registry,
};
use patches_dsl::ast::Span;
use patches_dsl::flat::{FlatConnection, FlatModule, FlatPatch, FlatPortRef, PortDirection};

/// Classification for a [`BindError`] — descriptor-level binding failures.
///
/// These codes share their `BN####` wire format with [`crate::InterpretErrorCode`]
/// so diagnostics consumers can treat both error families uniformly. Codes
/// covering runtime-only concerns (orphan-port graph lookup, tracker shape,
/// sequencer/song mismatch) are **not** present here — they stay in
/// [`crate::InterpretError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindErrorCode {
    /// Module type name not present in the registry.
    UnknownModuleType,
    /// Shape arguments were rejected by the registry's `describe`.
    InvalidShape,
    /// Parameter value type did not match the descriptor's expected kind.
    InvalidParameterType,
    /// Parameter name is not defined on the descriptor.
    UnknownParameter,
    /// Parameter conversion / range / enum variant failure.
    ParameterConversion,
    /// Module referenced in a connection / port-ref is absent from the patch.
    UnknownModule,
    /// Port referenced is absent from the descriptor.
    UnknownPort,
    /// Cable kind mismatch (mono ↔ poly) between connection endpoints.
    CableKindMismatch,
    /// Two connections drive the same input port — only one source is allowed.
    DuplicateInputConnection,
    /// Poly layout mismatch between connection endpoints (ADR 0033).
    PolyLayoutMismatch,
}

impl BindErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnknownModuleType => "BN0001",
            Self::InvalidShape => "BN0002",
            Self::InvalidParameterType => "BN0003",
            Self::UnknownParameter => "BN0004",
            Self::ParameterConversion => "BN0005",
            Self::UnknownModule => "BN0006",
            Self::UnknownPort => "BN0007",
            Self::CableKindMismatch => "BN0008",
            Self::DuplicateInputConnection => "BN0009",
            Self::PolyLayoutMismatch => "BN0012",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::UnknownModuleType => "unknown module type",
            Self::InvalidShape => "invalid shape",
            Self::InvalidParameterType => "invalid parameter type",
            Self::UnknownParameter => "unknown parameter",
            Self::ParameterConversion => "parameter conversion failed",
            Self::UnknownModule => "unknown module",
            Self::UnknownPort => "unknown port",
            Self::CableKindMismatch => "cable kind mismatch",
            Self::DuplicateInputConnection => "duplicate input connection",
            Self::PolyLayoutMismatch => "poly layout mismatch",
        }
    }
}

/// An error produced during descriptor-level binding.
///
/// Carries the [`Provenance`] of the offending construct plus a
/// human-readable message. Every error has a [`BindErrorCode`] so
/// diagnostics can dispatch without string-matching messages.
#[derive(Debug, Clone)]
pub struct BindError {
    pub code: BindErrorCode,
    pub provenance: Provenance,
    pub message: String,
}

impl BindError {
    pub fn new(
        code: BindErrorCode,
        provenance: Provenance,
        message: impl Into<String>,
    ) -> Self {
        Self { code, provenance, message: message.into() }
    }

    pub fn span(&self) -> Span {
        self.provenance.site
    }
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.provenance.site;
        write!(f, "{} (at {}..{})", self.message, s.start, s.end)
    }
}

impl std::error::Error for BindError {}

/// Typed failure mode from [`crate::convert_params`].
///
/// Replaces the previous string-substring classification so
/// [`BindErrorCode`] selection is a straight `match` on the variant.
/// Each variant carries the rendered message — kept byte-identical to
/// the previous `String` error so tests and diagnostics consumers are
/// unaffected.
#[derive(Debug, Clone)]
pub enum ParamConversionError {
    /// Parameter name is not defined on the descriptor.
    Unknown(String),
    /// Value kind disagrees with the descriptor's expected
    /// [`patches_core::ParameterKind`] (e.g. `int` where `float` was expected).
    TypeMismatch(String),
    /// Value is well-typed but outside the accepted range — invalid enum
    /// variant, unknown song reference, or unsupported file extension.
    OutOfRange(String),
}

impl ParamConversionError {
    pub fn message(&self) -> &str {
        match self {
            Self::Unknown(m) | Self::TypeMismatch(m) | Self::OutOfRange(m) => m.as_str(),
        }
    }

    pub fn into_message(self) -> String {
        match self {
            Self::Unknown(m) | Self::TypeMismatch(m) | Self::OutOfRange(m) => m,
        }
    }

    /// Wrap the inner message with a `"parameter '{name}': "` prefix,
    /// preserving the variant so `BindErrorCode` classification is
    /// unaffected.
    pub fn prefix_with_param(self, name: &str) -> Self {
        match self {
            Self::Unknown(m) => Self::Unknown(format!("parameter '{name}': {m}")),
            Self::TypeMismatch(m) => Self::TypeMismatch(format!("parameter '{name}': {m}")),
            Self::OutOfRange(m) => Self::OutOfRange(format!("parameter '{name}': {m}")),
        }
    }

    /// Map a typed conversion error to its descriptor-level [`BindErrorCode`].
    pub fn bind_code(&self) -> BindErrorCode {
        match self {
            Self::Unknown(_) => BindErrorCode::UnknownParameter,
            Self::TypeMismatch(_) => BindErrorCode::InvalidParameterType,
            Self::OutOfRange(_) => BindErrorCode::ParameterConversion,
        }
    }
}

impl std::fmt::Display for ParamConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// A [`FlatModule`] paired with its resolved [`ModuleDescriptor`] and
/// validated parameter map.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub id: QName,
    pub type_name: String,
    pub descriptor: ModuleDescriptor,
    pub params: ParameterMap,
    pub port_aliases: Vec<(u32, String)>,
    pub provenance: Provenance,
}

/// A [`FlatModule`] that could not be fully bound against the registry.
///
/// The raw flat fields are preserved so feature handlers (hover, completions)
/// can still offer user-visible diagnostics and partial information against
/// whatever *did* parse. `reason` classifies the first failure encountered
/// on this module; additional failures are recorded in [`BoundPatch::errors`].
#[derive(Debug, Clone)]
pub struct UnresolvedModule {
    pub id: QName,
    pub type_name: String,
    pub shape: Vec<(String, patches_dsl::ast::Scalar)>,
    pub params: Vec<(String, patches_dsl::ast::Value)>,
    pub port_aliases: Vec<(u32, String)>,
    pub provenance: Provenance,
    pub reason: BindErrorCode,
}

/// One module in a [`BoundPatch`]: either fully resolved against the
/// registry, or retained unresolved so downstream code can still walk the
/// graph.
#[derive(Debug, Clone)]
pub enum BoundModule {
    Resolved(ResolvedModule),
    Unresolved(UnresolvedModule),
}

impl BoundModule {
    pub fn id(&self) -> &QName {
        match self {
            Self::Resolved(m) => &m.id,
            Self::Unresolved(m) => &m.id,
        }
    }

    pub fn type_name(&self) -> &str {
        match self {
            Self::Resolved(m) => &m.type_name,
            Self::Unresolved(m) => &m.type_name,
        }
    }

    pub fn provenance(&self) -> &Provenance {
        match self {
            Self::Resolved(m) => &m.provenance,
            Self::Unresolved(m) => &m.provenance,
        }
    }

    pub fn as_resolved(&self) -> Option<&ResolvedModule> {
        match self {
            Self::Resolved(m) => Some(m),
            Self::Unresolved(_) => None,
        }
    }
}

/// A connection with both endpoints resolved against their respective
/// module descriptors.
#[derive(Debug, Clone)]
pub struct ResolvedConnection {
    pub from_module: QName,
    pub from_port: PortRef,
    pub from_kind: CableKind,
    pub from_layout: PolyLayout,
    pub to_module: QName,
    pub to_port: PortRef,
    pub to_kind: CableKind,
    pub to_layout: PolyLayout,
    pub scale: f64,
    pub provenance: Provenance,
}

/// A connection that could not be resolved (missing module, missing port,
/// or cable/layout mismatch).
#[derive(Debug, Clone)]
pub struct UnresolvedConnection {
    pub raw: FlatConnection,
    pub reason: BindErrorCode,
}

#[derive(Debug, Clone)]
pub enum BoundConnection {
    Resolved(ResolvedConnection),
    Unresolved(UnresolvedConnection),
}

impl BoundConnection {
    pub fn provenance(&self) -> &Provenance {
        match self {
            Self::Resolved(c) => &c.provenance,
            Self::Unresolved(c) => &c.raw.provenance,
        }
    }
}

/// A template-boundary port reference resolved against its module's
/// descriptor.
#[derive(Debug, Clone)]
pub struct ResolvedPortRef {
    pub module: QName,
    pub port: PortRef,
    pub direction: PortDirection,
    pub kind: CableKind,
    pub layout: PolyLayout,
    pub provenance: Provenance,
}

#[derive(Debug, Clone)]
pub struct UnresolvedPortRef {
    pub raw: FlatPortRef,
    pub reason: BindErrorCode,
}

#[derive(Debug, Clone)]
pub enum BoundPortRef {
    Resolved(ResolvedPortRef),
    Unresolved(UnresolvedPortRef),
}

impl BoundPortRef {
    pub fn provenance(&self) -> &Provenance {
        match self {
            Self::Resolved(r) => &r.provenance,
            Self::Unresolved(r) => &r.raw.provenance,
        }
    }
}

/// The result of descriptor-level binding.
///
/// Mirrors [`FlatPatch`]'s module/connection/port-ref structure with each
/// element tagged `Resolved` or `Unresolved`. Pattern and song definitions
/// are **not** duplicated here — they are consumed by [`crate::build`]
/// alongside a bound graph — callers that need them keep their original
/// [`FlatPatch`].
#[derive(Debug, Clone)]
pub struct BoundPatch {
    pub modules: Vec<BoundModule>,
    pub connections: Vec<BoundConnection>,
    pub port_refs: Vec<BoundPortRef>,
    pub errors: Vec<BindError>,
}

impl BoundPatch {
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }

    /// Look up a bound module by id.
    pub fn find_module(&self, id: &QName) -> Option<&BoundModule> {
        self.modules.iter().find(|m| m.id() == id)
    }
}

impl patches_dsl::pipeline::PipelineAudit for BoundPatch {
    /// Audit stage-3b [`BindError`]s for layering violations and return
    /// one [`LayeringWarning`](patches_dsl::pipeline::LayeringWarning)
    /// per violation. Called automatically by the pipeline orchestrator
    /// (`run_all` / `run_accumulate`) after bind so every consumer sees
    /// the same warnings without explicit opt-in.
    ///
    /// Currently flags [`BindErrorCode::UnknownModule`] — an unknown
    /// module reference reaching stage 3b means expansion (stage 3a)
    /// let a reference slip past its own unknown-module check. Future
    /// `PV####` codes land here without a signature change.
    fn layering_warnings(&self) -> Vec<patches_dsl::pipeline::LayeringWarning> {
        self.errors
            .iter()
            .filter_map(|e| match e.code {
                BindErrorCode::UnknownModule => {
                    Some(patches_dsl::pipeline::LayeringWarning {
                        code: "PV0001",
                        message: format!(
                            "stage 3b descriptor_bind reported '{}'; stage 3a expansion should have \
                             rejected this reference",
                            e.message
                        ),
                        span: e.span(),
                    })
                }
                _ => None,
            })
            .collect()
    }
}

/// Bind a [`FlatPatch`] against `registry`, producing a [`BoundPatch`].
///
/// Never returns `Err`: all failures are folded into
/// [`BoundPatch::errors`], with the raw flat elements preserved as
/// `Unresolved` variants so downstream passes can keep walking the graph.
pub fn bind(flat: &FlatPatch, registry: &Registry) -> BoundPatch {
    bind_with_base_dir(flat, registry, None)
}

/// Bind variant that resolves relative `File` / `String(path)` parameters
/// against `base_dir` while converting each module's params. Used by
/// consumers (player, CLAP) that load a patch from a `.patches` file on
/// disk; the LSP passes `None`.
pub fn bind_with_base_dir(
    flat: &FlatPatch,
    registry: &Registry,
    base_dir: Option<&std::path::Path>,
) -> BoundPatch {
    let song_name_to_index: HashMap<String, usize> = {
        let mut names: Vec<String> = flat.songs.iter().map(|s| s.name.to_string()).collect();
        names.sort();
        names.into_iter().enumerate().map(|(i, n)| (n, i)).collect()
    };

    let mut errors: Vec<BindError> = Vec::new();
    let mut modules: Vec<BoundModule> = Vec::with_capacity(flat.modules.len());

    for fm in &flat.modules {
        modules.push(bind_module(fm, registry, base_dir, &song_name_to_index, &mut errors));
    }

    // Index resolved modules by id for connection / port-ref lookup. Only
    // resolved modules participate — unresolved references downstream are
    // caught below via the `missing` branch.
    let by_id: HashMap<QName, &ResolvedModule> = modules
        .iter()
        .filter_map(|m| m.as_resolved())
        .map(|r| (r.id.clone(), r))
        .collect();

    let port_aliases: HashMap<QName, HashMap<u32, String>> = flat
        .modules
        .iter()
        .map(|m| (m.id.clone(), m.port_aliases.iter().cloned().collect()))
        .collect();

    let mut connections: Vec<BoundConnection> = Vec::with_capacity(flat.connections.len());
    for conn in &flat.connections {
        connections.push(bind_connection(conn, &by_id, &port_aliases, &mut errors));
    }

    // Each input port may be driven by at most one source. Detect duplicates
    // here so the LSP flags them; the engine's graph builder enforces the
    // same invariant at runtime (RT0001). Key on (module, port-name, index)
    // and report the *second* occurrence, leaving the first as authoritative.
    let mut seen_inputs: HashMap<(QName, &'static str, usize), &Provenance> = HashMap::new();
    for (bc, raw) in connections.iter().zip(flat.connections.iter()) {
        let BoundConnection::Resolved(rc) = bc else { continue };
        let key = (rc.to_module.clone(), rc.to_port.name, rc.to_port.index);
        if seen_inputs.insert(key, &raw.to_provenance).is_some() {
            errors.push(BindError::new(
                BindErrorCode::DuplicateInputConnection,
                raw.to_provenance.clone(),
                format!(
                    "input port '{}/{}' on module '{}' already has a connection",
                    rc.to_port.name, rc.to_port.index, rc.to_module
                ),
            ));
        }
    }

    let mut port_refs: Vec<BoundPortRef> = Vec::with_capacity(flat.port_refs.len());
    for pr in &flat.port_refs {
        port_refs.push(bind_port_ref(pr, &by_id, &port_aliases, &mut errors));
    }

    BoundPatch { modules, connections, port_refs, errors }
}

fn bind_module(
    fm: &FlatModule,
    registry: &Registry,
    base_dir: Option<&std::path::Path>,
    song_name_to_index: &HashMap<String, usize>,
    errors: &mut Vec<BindError>,
) -> BoundModule {
    let shape = crate::shape_from_args(&fm.shape);

    let descriptor = match registry.describe(&fm.type_name, &shape) {
        Ok(d) => d,
        Err(e) => {
            // Disambiguate unknown-type vs shape rejection by looking at the
            // error payload. `Registry::describe` returns `BuildError::Custom`
            // (or a specific variant) — we keep the message as-is and pick
            // the narrower code when the type isn't registered.
            let code = if registry.module_names().any(|n| n == fm.type_name) {
                BindErrorCode::InvalidShape
            } else {
                BindErrorCode::UnknownModuleType
            };
            errors.push(BindError::new(code, fm.provenance.clone(), e.to_string()));
            return mark_unresolved(fm, code);
        }
    };

    let params = match crate::convert_params(&fm.params, &descriptor, base_dir, song_name_to_index) {
        Ok(p) => p,
        Err(err) => {
            let code = err.bind_code();
            errors.push(BindError::new(code, fm.provenance.clone(), err.into_message()));
            return mark_unresolved(fm, code);
        }
    };

    if let Err(e) = patches_core::validate_parameters(&params, &descriptor) {
        errors.push(BindError::new(
            BindErrorCode::ParameterConversion,
            fm.provenance.clone(),
            e.to_string(),
        ));
        return mark_unresolved(fm, BindErrorCode::ParameterConversion);
    }

    BoundModule::Resolved(ResolvedModule {
        id: fm.id.clone(),
        type_name: fm.type_name.clone(),
        descriptor,
        params,
        port_aliases: fm.port_aliases.clone(),
        provenance: fm.provenance.clone(),
    })
}

/// Build an `Unresolved` [`BoundModule`] tagged with `code`, preserving the
/// raw flat fields so downstream consumers (hover, completions) can still
/// surface partial information. Extracted from three identical inline
/// blocks in [`bind_module`] — ticket 0445.
fn mark_unresolved(fm: &FlatModule, code: BindErrorCode) -> BoundModule {
    BoundModule::Unresolved(UnresolvedModule {
        id: fm.id.clone(),
        type_name: fm.type_name.clone(),
        shape: fm.shape.clone(),
        params: fm.params.clone(),
        port_aliases: fm.port_aliases.clone(),
        provenance: fm.provenance.clone(),
        reason: code,
    })
}

fn bind_connection(
    conn: &FlatConnection,
    by_id: &HashMap<QName, &ResolvedModule>,
    port_aliases: &HashMap<QName, HashMap<u32, String>>,
    errors: &mut Vec<BindError>,
) -> BoundConnection {
    let Some(from) = by_id.get(&conn.from_module).copied() else {
        errors.push(BindError::new(
            BindErrorCode::UnknownModule,
            conn.from_provenance.clone(),
            format!("module '{}' not found", conn.from_module),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::UnknownModule,
        });
    };
    let Some(to) = by_id.get(&conn.to_module).copied() else {
        errors.push(BindError::new(
            BindErrorCode::UnknownModule,
            conn.to_provenance.clone(),
            format!("module '{}' not found", conn.to_module),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::UnknownModule,
        });
    };

    let from_port_desc = find_port(&from.descriptor.outputs, &conn.from_port, conn.from_index);
    let to_port_desc = find_port(&to.descriptor.inputs, &conn.to_port, conn.to_index);

    let from_port_desc = match from_port_desc {
        Some(p) => p,
        None => {
            let aliases = port_aliases.get(&conn.from_module);
            errors.push(BindError::new(
                BindErrorCode::UnknownPort,
                conn.from_provenance.clone(),
                format!(
                    "module '{}' has no output port '{}'; available outputs: [{}]",
                    conn.from_module,
                    crate::format_port_label(&conn.from_port, conn.from_index, aliases),
                    crate::format_available_ports(&from.descriptor.outputs, aliases),
                ),
            ));
            return BoundConnection::Unresolved(UnresolvedConnection {
                raw: conn.clone(),
                reason: BindErrorCode::UnknownPort,
            });
        }
    };
    let to_port_desc = match to_port_desc {
        Some(p) => p,
        None => {
            let aliases = port_aliases.get(&conn.to_module);
            errors.push(BindError::new(
                BindErrorCode::UnknownPort,
                conn.to_provenance.clone(),
                format!(
                    "module '{}' has no input port '{}'; available inputs: [{}]",
                    conn.to_module,
                    crate::format_port_label(&conn.to_port, conn.to_index, aliases),
                    crate::format_available_ports(&to.descriptor.inputs, aliases),
                ),
            ));
            return BoundConnection::Unresolved(UnresolvedConnection {
                raw: conn.clone(),
                reason: BindErrorCode::UnknownPort,
            });
        }
    };

    // Cable kind must match exactly.
    if from_port_desc.kind != to_port_desc.kind {
        errors.push(BindError::new(
            BindErrorCode::CableKindMismatch,
            conn.provenance.clone(),
            format!(
                "cable kind mismatch: '{}.{}' ({:?}) → '{}.{}' ({:?})",
                conn.from_module, conn.from_port, from_port_desc.kind,
                conn.to_module, conn.to_port, to_port_desc.kind,
            ),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::CableKindMismatch,
        });
    }

    // Poly layout compatibility (mono-mono is trivially compatible).
    if from_port_desc.kind == CableKind::Poly
        && !from_port_desc.poly_layout.compatible_with(to_port_desc.poly_layout)
    {
        errors.push(BindError::new(
            BindErrorCode::PolyLayoutMismatch,
            conn.provenance.clone(),
            format!(
                "poly layout mismatch: '{}.{}' ({:?}) → '{}.{}' ({:?})",
                conn.from_module, conn.from_port, from_port_desc.poly_layout,
                conn.to_module, conn.to_port, to_port_desc.poly_layout,
            ),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::PolyLayoutMismatch,
        });
    }

    BoundConnection::Resolved(ResolvedConnection {
        from_module: conn.from_module.clone(),
        from_port: PortRef { name: from_port_desc.name, index: from_port_desc.index },
        from_kind: from_port_desc.kind.clone(),
        from_layout: from_port_desc.poly_layout,
        to_module: conn.to_module.clone(),
        to_port: PortRef { name: to_port_desc.name, index: to_port_desc.index },
        to_kind: to_port_desc.kind.clone(),
        to_layout: to_port_desc.poly_layout,
        scale: conn.scale,
        provenance: conn.provenance.clone(),
    })
}

fn bind_port_ref(
    pr: &FlatPortRef,
    by_id: &HashMap<QName, &ResolvedModule>,
    port_aliases: &HashMap<QName, HashMap<u32, String>>,
    errors: &mut Vec<BindError>,
) -> BoundPortRef {
    let Some(owner) = by_id.get(&pr.module).copied() else {
        errors.push(BindError::new(
            BindErrorCode::UnknownModule,
            pr.provenance.clone(),
            format!("module '{}' not found", pr.module),
        ));
        return BoundPortRef::Unresolved(UnresolvedPortRef {
            raw: pr.clone(),
            reason: BindErrorCode::UnknownModule,
        });
    };

    let (ports, kind_str) = match pr.direction {
        PortDirection::Output => (&owner.descriptor.outputs[..], "output"),
        PortDirection::Input => (&owner.descriptor.inputs[..], "input"),
    };
    let Some(desc) = find_port(ports, &pr.port, pr.index) else {
        let aliases = port_aliases.get(&pr.module);
        errors.push(BindError::new(
            BindErrorCode::UnknownPort,
            pr.provenance.clone(),
            format!(
                "module '{}' has no {} port '{}'; available {}s: [{}]",
                pr.module,
                kind_str,
                crate::format_port_label(&pr.port, pr.index, aliases),
                kind_str,
                crate::format_available_ports(ports, aliases),
            ),
        ));
        return BoundPortRef::Unresolved(UnresolvedPortRef {
            raw: pr.clone(),
            reason: BindErrorCode::UnknownPort,
        });
    };

    BoundPortRef::Resolved(ResolvedPortRef {
        module: pr.module.clone(),
        port: PortRef { name: desc.name, index: desc.index },
        direction: pr.direction,
        kind: desc.kind.clone(),
        layout: desc.poly_layout,
        provenance: pr.provenance.clone(),
    })
}

fn find_port<'a>(
    ports: &'a [PortDescriptor],
    name: &str,
    index: u32,
) -> Option<&'a PortDescriptor> {
    ports.iter().find(|p| p.name == name && p.index == index as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::Provenance as CoreProv;
    use patches_dsl::ast::{Scalar, SourceId, Span, Value};
    use patches_dsl::flat::{FlatConnection, FlatModule, FlatPatch};

    fn syn() -> Span {
        Span::synthetic()
    }

    fn registry() -> Registry {
        patches_modules::default_registry()
    }

    fn empty_flat() -> FlatPatch {
        FlatPatch {
            modules: vec![],
            connections: vec![],
            patterns: vec![],
            songs: vec![],
            port_refs: vec![],
        }
    }

    fn osc(id: &str) -> FlatModule {
        FlatModule {
            id: id.into(),
            type_name: "Osc".into(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
        }
    }

    fn sum(id: &str, channels: i64) -> FlatModule {
        FlatModule {
            id: id.into(),
            type_name: "Sum".into(),
            shape: vec![("channels".into(), Scalar::Int(channels))],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
        }
    }

    fn conn(from: &str, fp: &str, to: &str, tp: &str) -> FlatConnection {
        let prov = CoreProv::root(syn());
        FlatConnection {
            from_module: from.into(),
            from_port: fp.into(),
            from_index: 0,
            to_module: to.into(),
            to_port: tp.into(),
            to_index: 0,
            scale: 1.0,
            provenance: prov.clone(),
            from_provenance: prov.clone(),
            to_provenance: prov,
        }
    }

    #[test]
    fn clean_patch_has_no_errors() {
        let mut flat = empty_flat();
        flat.modules = vec![osc("o1"), sum("mix", 1)];
        flat.connections = vec![conn("o1", "sine", "mix", "in")];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);
        assert!(matches!(bound.modules[0], BoundModule::Resolved(_)));
        assert!(matches!(bound.connections[0], BoundConnection::Resolved(_)));
    }

    #[test]
    fn unknown_type_produces_unresolved_module() {
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "x".into(),
            type_name: "NoSuch".into(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(Span::new(SourceId::SYNTHETIC, 5, 10)),
        }];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::UnknownModuleType);
        assert!(matches!(bound.modules[0], BoundModule::Unresolved(_)));
    }

    #[test]
    fn unknown_port_produces_unresolved_connection() {
        let mut flat = empty_flat();
        flat.modules = vec![osc("o1"), sum("mix", 1)];
        flat.connections = vec![conn("o1", "no_such_out", "mix", "in")];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::UnknownPort);
        assert!(matches!(bound.connections[0], BoundConnection::Unresolved(_)));
    }

    #[test]
    fn unknown_module_in_connection_classified() {
        let mut flat = empty_flat();
        flat.modules = vec![sum("mix", 1)];
        flat.connections = vec![conn("ghost", "out", "mix", "in")];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::UnknownModule);
    }

    #[test]
    fn accumulates_multiple_errors() {
        let mut flat = empty_flat();
        flat.modules = vec![
            FlatModule {
                id: "a".into(),
                type_name: "NoSuch1".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: CoreProv::root(syn()),
            },
            FlatModule {
                id: "b".into(),
                type_name: "NoSuch2".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: CoreProv::root(syn()),
            },
        ];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 2);
    }

    #[test]
    fn unknown_parameter_reports_code() {
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "o1".into(),
            type_name: "Osc".into(),
            shape: vec![],
            params: vec![("no_such_param".into(), Value::Scalar(Scalar::Float(1.0)))],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
        }];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::UnknownParameter);
    }

    // ── Typed parameter error classification (ticket 0441) ──────────────

    #[test]
    fn param_conversion_unknown_maps_to_unknown_parameter() {
        let err = ParamConversionError::Unknown("unknown parameter 'x'".into());
        assert_eq!(err.bind_code(), BindErrorCode::UnknownParameter);
    }

    #[test]
    fn param_conversion_type_mismatch_maps_to_invalid_parameter_type() {
        let err = ParamConversionError::TypeMismatch("expected float, found string".into());
        assert_eq!(err.bind_code(), BindErrorCode::InvalidParameterType);
    }

    #[test]
    fn param_conversion_out_of_range_maps_to_parameter_conversion() {
        let err = ParamConversionError::OutOfRange("invalid enum variant 'foo'".into());
        assert_eq!(err.bind_code(), BindErrorCode::ParameterConversion);
    }

    #[test]
    fn bind_classifies_type_mismatch_without_substring_matching() {
        // Osc's `frequency` is a float; passing a bool triggers the
        // `expected …, found …` branch of convert_value.
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "o1".into(),
            type_name: "Osc".into(),
            shape: vec![],
            params: vec![(
                "frequency".into(),
                patches_dsl::ast::Value::Scalar(patches_dsl::ast::Scalar::Bool(true)),
            )],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
        }];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::InvalidParameterType);
    }
}
