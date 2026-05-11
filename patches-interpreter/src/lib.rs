//! `patches-interpreter` — validates and builds a [`ModuleGraph`] from a
//! [`patches_dsl::FlatPatch`].
//!
//! # Responsibilities
//!
//! - Holds the module factory registry (type name → descriptor + builder).
//! - Resolves module type names from the flat AST against the registry.
//! - Validates shape args, port references, and parameter values against
//!   the module's [`ModuleDescriptor`].
//! - Calls [`ModuleGraph::add_module`] and [`ModuleGraph::connect`] to
//!   construct the runtime graph.
//! - Collects pattern and song blocks into [`TrackerData`] for sequencer
//!   modules.
//! - Propagates source spans from the AST into error messages.
//!
//! This crate has no concrete module-type, audio-backend, or engine
//! dependencies; callers pass in a `&Registry` populated however they
//! like (in-tree modules, manifest-backed, plugin scan, …).

mod binding;
pub mod descriptor_bind;
mod error;
mod tracker;

pub use descriptor_bind::{
    bind, bind_with_base_dir, BindError, BindErrorCode, BoundConnection, BoundGraph, BoundModule,
    BoundPatch, BoundPortRef, ParamConversionError, ResolvedConnection, ResolvedModule,
    ResolvedPortRef, UnresolvedConnection, UnresolvedModule, UnresolvedPortRef,
};
pub use error::{BuildError, BuildErrorSource, InterpretError, InterpretErrorCode};

use std::collections::HashMap;
use std::path::Path;

use patches_core::{AudioEnvironment, ModuleGraph, TrackerData};
use patches_core::registry::Registry;
use patches_dsl::ast::{Scalar, Value};
use patches_dsl::flat::FlatPatch;

use binding::require_resolved;
use tracker::{build_tracker_data, convert_value};

/// The result of interpreting a [`FlatPatch`]: a module graph and optional
/// tracker data (patterns and songs).
pub struct BuildResult {
    pub graph: ModuleGraph,
    pub tracker_data: Option<TrackerData>,
}

impl patches_dsl::pipeline::PipelineAudit for BuildResult {}

impl std::fmt::Debug for BuildResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildResult")
            .field("graph", &format_args!("ModuleGraph({} nodes)", self.graph.node_ids().len()))
            .field("tracker_data", &self.tracker_data)
            .finish()
    }
}

/// Build a [`ModuleGraph`] (and optional [`TrackerData`]) from a validated
/// [`FlatPatch`].
///
/// Module type names are resolved against `registry`. Shape args and
/// parameter values are validated against each module's
/// [`patches_core::ModuleDescriptor`]. Connection port names are validated
/// against the descriptors already added to the graph, so forward references
/// within a single patch are not errors.
///
/// Returns an [`InterpretError`] with the source span of the offending
/// declaration on the first validation failure encountered. On error, any
/// partially-constructed graph is discarded — callers must not attempt to
/// recover from a half-built state.
pub fn build(
    flat: &FlatPatch,
    registry: &Registry,
    env: &AudioEnvironment,
) -> Result<BuildResult, BuildError> {
    build_with_base_dir(flat, registry, env, None)
}

/// Convenience: [`descriptor_bind::bind_with_base_dir`] followed by
/// [`build_from_bound`]. Fails on the first [`BindError`] or
/// [`InterpretError`] encountered. Consumers that want to surface every
/// bind error for a user should drive the two stages explicitly and
/// render [`BoundPatch::errors`] before handing the bound graph to
/// [`build_from_bound`].
pub fn build_with_base_dir(
    flat: &FlatPatch,
    registry: &Registry,
    env: &AudioEnvironment,
    base_dir: Option<&Path>,
) -> Result<BuildResult, BuildError> {
    let bound = bind_with_base_dir(flat, registry, base_dir);
    if let Some(first) = bound.errors.first() {
        return Err(BuildError::from_bind(first));
    }
    build_from_bound(&bound, env).map_err(BuildError::from_interpret)
}

/// Build a [`ModuleGraph`] (and optional [`TrackerData`]) from a
/// [`BoundPatch`] (produced by [`descriptor_bind::bind_with_base_dir`]).
///
/// The caller is responsible for having checked [`BoundPatch::errors`];
/// unresolved modules are skipped — if a referenced module is missing a
/// descriptor, this function returns an [`InterpretError::Other`]
/// rather than swallowing the violation. [`BoundPatch::song_data`] carries
/// the pattern and song definitions threaded through bind unchanged.
pub fn build_from_bound(
    bound: &BoundPatch,
    _env: &AudioEnvironment,
) -> Result<BuildResult, InterpretError> {
    let mut graph = ModuleGraph::new();

    // Stage 1 — add module nodes directly from the bound graph's
    // resolved descriptors + parameter maps. `require_resolved` is
    // defensive: the caller must have short-circuited on bound.errors.
    for bm in &bound.modules {
        let resolved = require_resolved(bm, "module")?;
        graph
            .add_module_with_structural(
                resolved.id.clone(),
                resolved.descriptor.clone(),
                &resolved.params,
                &resolved.structural,
            )
            .map_err(|e| {
                InterpretError::new(
                    InterpretErrorCode::ConnectFailed,
                    resolved.provenance.clone(),
                    e.to_string(),
                )
            })?;
    }

    // Stage 2 — connect from the bound graph's resolved connections.
    // `require_resolved` is defensive: the caller must have short-circuited
    // on bound.errors.
    for bc in &bound.connections {
        let resolved = require_resolved(bc, "connection")?;
        let from_id = patches_core::NodeId::from(resolved.from_module.clone());
        let to_id = patches_core::NodeId::from(resolved.to_module.clone());
        let cable_map = patches_core::CableMap {
            scale: resolved.map.scale as f32,
            offset: resolved.map.offset as f32,
            clip: resolved.map.clip.map(|(lo, hi)| (lo as f32, hi as f32)),
        };
        graph
            .connect_with_map(
                &from_id,
                resolved.from_port,
                &to_id,
                resolved.to_port,
                cable_map,
            )
            .map_err(|e| {
                InterpretError::new(
                    InterpretErrorCode::ConnectFailed,
                    resolved.provenance.clone(),
                    e.to_string(),
                )
            })?;
    }

    // Stage 2.5 — template-boundary port refs are already validated at
    // bind time (port existence + direction). Confirm the owning module
    // made it into the runtime graph; a missing node here is a
    // pipeline-layering failure, not a user error, but we still surface
    // it so the caller notices.
    // `require_resolved` is defensive: the caller must have short-circuited
    // on bound.errors.
    for pr in &bound.port_refs {
        let resolved = require_resolved(pr, "port_ref")?;
        let id = patches_core::NodeId::from(resolved.module.clone());
        if graph.get_node(&id).is_none() {
            return Err(InterpretError::new(
                InterpretErrorCode::OrphanPortRef,
                resolved.provenance.clone(),
                format!(
                    "module '{}' referenced by template-boundary port ref is not in the graph",
                    resolved.module
                ),
            ));
        }
    }

    // Stage 3 — build tracker data from pattern and song blocks.
    let tracker_data = build_tracker_data(&bound.song_data, &bound.graph.modules)?;

    Ok(BuildResult { graph, tracker_data })
}

// ── Shared descriptor-resolution helpers ────────────────────────────────────
//
// Shape/parameter/port-label helpers consumed by [`descriptor_bind`] live
// in this block. After ticket 0438, [`build_from_bound`] no longer calls
// them (the bound graph already carries resolved descriptors and
// validated parameter maps); they exist here only because splitting them
// across `lib` and `descriptor_bind` risked drift between the two passes.

/// Convert `Vec<(String, Scalar)>` shape arguments to a [`ModuleShape`].
///
/// Recognised keys are `"channels"` and `"length"`; unrecognised keys are
/// silently ignored (the registry's `describe` implementation is responsible
/// for validating shape semantics).
pub(crate) fn shape_from_args(args: &[(String, Scalar)]) -> patches_core::ModuleShape {
    let mut shape = patches_core::ModuleShape::default();
    for (name, scalar) in args {
        if name.as_str() == "channels" {
            if let Scalar::Int(n) = scalar {
                shape.channels = *n as usize;
            }
        }
        // Other keys (former `length`, `high_quality`) are now structural
        // params and travel via the params block (ADR 0060, ticket 0738).
    }
    shape
}

/// Format a single `port[alias]` (when alias known) or `port/index` label.
pub(crate) fn format_port_label(
    port: &str,
    index: u32,
    aliases: Option<&HashMap<u32, String>>,
) -> String {
    match aliases.and_then(|m| m.get(&index)) {
        Some(alias) => format!("{}[{}]", port, alias),
        None => format!("{}/{}", port, index),
    }
}

/// Format the bracketed `[port[alias], ...]` list of available ports for an
/// error message.
pub(crate) fn format_available_ports(
    ports: &[patches_core::PortDescriptor],
    aliases: Option<&HashMap<u32, String>>,
) -> String {
    ports
        .iter()
        .map(|p| format_port_label(p.name, p.index as u32, aliases))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parse a parameter name string of the form `"name"` or `"name/N"` into a
/// base name and index.
pub(crate) fn parse_param_name(name: &str) -> (&str, usize) {
    if let Some(pos) = name.rfind('/') {
        let base = &name[..pos];
        let idx_str = &name[pos + 1..];
        if let Ok(idx) = idx_str.parse::<usize>() {
            return (base, idx);
        }
    }
    (name, 0)
}

/// Convert a slice of `(name, Value)` DSL param pairs into a
/// realtime [`patches_core::ParameterMap`] paired with a
/// [`patches_core::StructuralParams`] carrier (ADR 0060). Each pair is
/// routed by descriptor: names declared in `realtime_params` land in the
/// `ParameterMap`; names declared in `structural_params` land in the
/// `StructuralParams`. Returns `Err` on the first type incompatibility or
/// unrecognised parameter name encountered.
///
/// `base_dir` resolves relative file paths declared via `file("…")` against
/// the source `.patches` file's directory, so the absolute path threaded to
/// `Module::prepare` does not depend on the engine's cwd.
pub(crate) fn convert_params(
    params: &[(String, Value)],
    descriptor: &patches_core::ModuleDescriptor,
    base_dir: Option<&Path>,
    song_name_to_index: &HashMap<String, usize>,
) -> Result<(patches_core::ParameterMap, patches_core::StructuralParams), ParamConversionError> {
    use patches_core::{ParameterMap, StructuralParams};
    let mut realtime = ParameterMap::new();
    let mut structural = StructuralParams::new();
    for (raw_name, value) in params {
        let (base_name, idx) = parse_param_name(raw_name);

        if let Some(rt) = descriptor
            .realtime_params
            .iter()
            .find(|p| p.name == base_name && p.index == idx)
        {
            let pv = convert_value(value, &rt.parameter_type, song_name_to_index)
                .map_err(|e| e.prefix_with_param(raw_name))?;
            realtime.insert_param(base_name.to_string(), idx, pv);
            continue;
        }

        if let Some(sp) = descriptor
            .structural_params
            .iter()
            .find(|p| p.name == base_name && p.index == idx)
        {
            let sv = convert_structural_value(
                value,
                &sp.parameter_type,
                base_dir,
                raw_name,
            )?;
            structural.insert(base_name.to_string(), idx, sv);
            continue;
        }

        let mut known: Vec<String> = descriptor
            .realtime_params
            .iter()
            .chain(descriptor.structural_params.iter())
            .map(|p| {
                if p.index == 0 {
                    p.name.to_string()
                } else {
                    format!("{}/{}", p.name, p.index)
                }
            })
            .collect();
        known.sort();
        known.dedup();
        return Err(ParamConversionError::unknown(format!(
            "unknown parameter '{raw_name}'; known parameters: {}",
            known.join(", ")
        )));
    }
    Ok((realtime, structural))
}

/// Convert a DSL [`Value`] for a structural parameter declared on the
/// descriptor (ADR 0060). `Float`/`Int`/`Bool` mirror the realtime
/// converters; `File`/`String` route into [`StructuralValue::String`].
/// Relative paths declared via `file("…")` are resolved against `base_dir`
/// when present.
fn convert_structural_value(
    value: &Value,
    kind: &patches_core::ParameterKind,
    base_dir: Option<&Path>,
    raw_name: &str,
) -> Result<patches_core::StructuralValue, ParamConversionError> {
    use patches_core::{ParameterKind, StructuralValue};
    match (value, kind) {
        (Value::Scalar(Scalar::Float(f)), ParameterKind::Float { .. }) => {
            Ok(StructuralValue::Float(*f as f32))
        }
        (Value::Scalar(Scalar::Int(i)), ParameterKind::Float { .. }) => {
            Ok(StructuralValue::Float(*i as f32))
        }
        (Value::Scalar(Scalar::Int(i)), ParameterKind::Int { .. }) => {
            Ok(StructuralValue::Int(*i))
        }
        (Value::Scalar(Scalar::Bool(b)), ParameterKind::Bool { .. }) => {
            Ok(StructuralValue::Bool(*b))
        }
        (Value::File(p), ParameterKind::File { extensions }) => {
            let resolved = resolve_file_path(p, base_dir);
            validate_file_extension(&resolved, extensions, raw_name)?;
            Ok(StructuralValue::String(resolved))
        }
        (Value::Scalar(Scalar::Str(s)), ParameterKind::File { extensions }) => {
            let resolved = resolve_file_path(s, base_dir);
            validate_file_extension(&resolved, extensions, raw_name)?;
            Ok(StructuralValue::String(resolved))
        }
        _ => Err(ParamConversionError::type_mismatch(format!(
            "parameter '{raw_name}': expected structural {}, found {}",
            kind.kind_name(),
            value_kind_name(value),
        ))),
    }
}

fn resolve_file_path(path: &str, base_dir: Option<&Path>) -> String {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return path.to_string();
    }
    match base_dir {
        Some(base) => base.join(p).to_string_lossy().into_owned(),
        None => path.to_string(),
    }
}

fn validate_file_extension(
    path: &str,
    extensions: &[&str],
    raw_name: &str,
) -> Result<(), ParamConversionError> {
    if extensions.is_empty() {
        return Ok(());
    }
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext {
        Some(ref e) if extensions.iter().any(|x| x.eq_ignore_ascii_case(e)) => Ok(()),
        _ => Err(ParamConversionError::out_of_range(format!(
            "parameter '{raw_name}': file '{path}' does not have an accepted extension ({})",
            extensions.join(", "),
        ))),
    }
}

fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Scalar(Scalar::Float(_)) => "float",
        Value::Scalar(Scalar::Int(_)) => "int",
        Value::Scalar(Scalar::Bool(_)) => "bool",
        Value::Scalar(Scalar::Str(_)) => "string",
        Value::Scalar(Scalar::ParamRef(_)) => "param-ref",
        Value::File(_) => "file",
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
