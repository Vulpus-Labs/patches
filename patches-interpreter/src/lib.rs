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
//! This crate knows about concrete module types (via `patches-modules`) but
//! has no audio-backend or engine dependencies.

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
use patches_registry::Registry;
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
            .add_module(
                resolved.id.clone(),
                resolved.descriptor.clone(),
                &resolved.params,
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
        graph
            .connect(
                &from_id,
                resolved.from_port,
                &to_id,
                resolved.to_port,
                resolved.scale as f32,
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
    let mut channels = 0usize;
    let mut length = 0usize;
    let mut high_quality = false;
    for (name, scalar) in args {
        match name.as_str() {
            "channels" => {
                if let Scalar::Int(n) = scalar {
                    channels = *n as usize;
                }
            }
            "length" => {
                if let Scalar::Int(n) = scalar {
                    length = *n as usize;
                }
            }
            "high_quality" => {
                if let Scalar::Bool(b) = scalar {
                    high_quality = *b;
                }
            }
            _ => {}
        }
    }
    patches_core::ModuleShape { channels, length, high_quality }
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
/// [`patches_core::ParameterMap`], validating each value's type against
/// the module's descriptor. Returns `Err(message)` on the first type
/// incompatibility or unrecognised parameter name encountered.
pub(crate) fn convert_params(
    params: &[(String, Value)],
    descriptor: &patches_core::ModuleDescriptor,
    base_dir: Option<&Path>,
    song_name_to_index: &HashMap<String, usize>,
) -> Result<patches_core::ParameterMap, ParamConversionError> {
    use patches_core::{ParameterKind, ParameterMap, ParameterValue};
    let mut map = ParameterMap::new();
    for (raw_name, value) in params {
        let (base_name, idx) = parse_param_name(raw_name);

        let param_desc = descriptor
            .parameters
            .iter()
            .find(|p| p.name == base_name && p.index == idx)
            .ok_or_else(|| {
                let mut known: Vec<String> = descriptor
                    .parameters
                    .iter()
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
                ParamConversionError::Unknown(format!(
                    "unknown parameter '{raw_name}'; known parameters: {}",
                    known.join(", ")
                ))
            })?;

        let mut pv = convert_value(value, &param_desc.parameter_type, song_name_to_index)
            .map_err(|e| e.prefix_with_param(raw_name))?;

        // Resolve relative file paths against the patch file's directory.
        if let Some(dir) = base_dir {
            match &mut pv {
                ParameterValue::File(s) if !s.is_empty() && !Path::new(s.as_str()).is_absolute() => {
                    *s = dir.join(s.as_str()).to_string_lossy().into_owned();
                }
                ParameterValue::String(s)
                    if matches!(param_desc.parameter_type, ParameterKind::String { .. })
                        && param_desc.name == "path"
                        && !s.is_empty()
                        && !Path::new(s.as_str()).is_absolute() =>
                {
                    *s = dir.join(s.as_str()).to_string_lossy().into_owned();
                }
                _ => {}
            }
        }

        map.insert_param(base_name.to_string(), idx, pv);
    }
    Ok(map)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
