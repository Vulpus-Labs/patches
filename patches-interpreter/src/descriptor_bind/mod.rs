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

pub mod connections;
pub mod errors;
pub mod modules;

pub use connections::{
    BoundConnection, BoundPortRef, ResolvedConnection, ResolvedPortRef, UnresolvedConnection,
    UnresolvedPortRef,
};
pub use errors::{BindError, BindErrorCode, ParamConversionError};
pub use modules::{BoundModule, ResolvedModule, UnresolvedModule};

use std::collections::HashMap;

use patches_core::QName;
use patches_registry::Registry;
use patches_dsl::flat::{FlatPatch, SongData};

use connections::{bind_connection, bind_port_ref};
use modules::bind_module;

/// Graph-relevant half of a [`BoundPatch`]: bound modules, connections,
/// port refs, and accumulated [`BindError`]s. Mirrors
/// [`patches_dsl::flat::FlatGraph`]'s structure with each element tagged
/// `Resolved` or `Unresolved`.
#[derive(Debug, Clone, Default)]
pub struct BoundGraph {
    pub modules: Vec<BoundModule>,
    pub connections: Vec<BoundConnection>,
    pub port_refs: Vec<BoundPortRef>,
    pub errors: Vec<BindError>,
}

impl BoundGraph {
    /// Look up a bound module by id.
    pub fn find_module(&self, id: &QName) -> Option<&BoundModule> {
        self.modules.iter().find(|m| m.id() == id)
    }
}

/// The result of descriptor-level binding.
///
/// Decomposes into a [`BoundGraph`] (modules/connections/port refs/errors)
/// and a [`SongData`] threaded through from the [`FlatPatch`] unchanged.
/// Pattern/song-based tracker data is built later by [`crate::build`].
#[derive(Debug, Clone, Default)]
pub struct BoundPatch {
    pub graph: BoundGraph,
    pub song_data: SongData,
}

impl std::ops::Deref for BoundPatch {
    type Target = BoundGraph;
    fn deref(&self) -> &BoundGraph {
        &self.graph
    }
}

impl std::ops::DerefMut for BoundPatch {
    fn deref_mut(&mut self) -> &mut BoundGraph {
        &mut self.graph
    }
}

impl BoundPatch {
    pub fn is_clean(&self) -> bool {
        self.graph.errors.is_empty()
    }

    /// Look up a bound module by id.
    pub fn find_module(&self, id: &QName) -> Option<&BoundModule> {
        self.graph.find_module(id)
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
        self.graph
            .errors
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
        let mut names: Vec<String> =
            flat.song_data.songs.iter().map(|s| s.name.to_string()).collect();
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
    let mut seen_inputs: HashMap<(QName, &'static str, usize), &patches_core::Provenance> =
        HashMap::new();
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

    BoundPatch {
        graph: BoundGraph { modules, connections, port_refs, errors },
        song_data: flat.song_data.clone(),
    }
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
        FlatPatch::default()
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
