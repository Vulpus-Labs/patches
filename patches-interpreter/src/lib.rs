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

pub mod descriptor_bind;

pub use descriptor_bind::{
    bind, bind_with_base_dir, BindError, BindErrorCode, BoundConnection, BoundModule, BoundPatch,
    BoundPortRef, ParamConversionError, ResolvedConnection, ResolvedModule, ResolvedPortRef,
    UnresolvedConnection, UnresolvedModule, UnresolvedPortRef,
};

use std::collections::HashMap;
use std::path::Path;

use patches_core::{
    AudioEnvironment, ModuleGraph, Registry,
    TrackerData, PatternBank, SongBank, Pattern, Song, TrackerStep,
};
use patches_core::Provenance;
use patches_dsl::ast::{Scalar, Span, Value};
use patches_dsl::flat::FlatPatch;

/// Classification for an [`InterpretError`] — stage 3b *runtime* graph
/// construction.
///
/// Ticket 0438 narrowed this enum to the runtime concerns that remain
/// inside [`build`] after descriptor-level binding moved to
/// [`descriptor_bind::bind`]. Every descriptor-level failure
/// (unknown module type, shape rejection, param type/range, unknown
/// port, cable/layout mismatch) now surfaces as a
/// [`descriptor_bind::BindError`] via [`BoundPatch::errors`]; callers
/// inspect that list and short-circuit before invoking
/// [`build_from_bound`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterpretErrorCode {
    /// [`ModuleGraph::connect`] rejected the connection (already-connected
    /// input, duplicate-id, scale out of range, arity mismatch).
    ConnectFailed,
    /// Template-boundary port ref did not resolve against the built graph.
    OrphanPortRef,
    /// Song/pattern shape inconsistency discovered while assembling
    /// tracker data.
    TrackerShape,
    /// `MasterSequencer` references an unknown song, or channel count
    /// disagrees with the song's column count.
    SequencerSongMismatch,
    #[default]
    Other,
}

impl InterpretErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ConnectFailed => "RT0001",
            Self::OrphanPortRef => "RT0002",
            Self::TrackerShape => "RT0003",
            Self::SequencerSongMismatch => "RT0004",
            Self::Other => "RT9999",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::ConnectFailed => "connect failed",
            Self::OrphanPortRef => "orphan port reference",
            Self::TrackerShape => "tracker shape mismatch",
            Self::SequencerSongMismatch => "sequencer/song mismatch",
            Self::Other => "runtime build error",
        }
    }
}

/// An error produced during interpretation of a [`FlatPatch`].
///
/// Carries the [`Provenance`] of the offending construct (innermost site plus
/// the chain of template call sites that led there) and a human-readable
/// message describing the problem. Every error has an
/// [`InterpretErrorCode`] so diagnostics can dispatch without
/// string-matching messages.
///
/// `span` returns the innermost site (`provenance.site`) for callers that
/// only care about the immediate location.
#[derive(Debug)]
pub struct InterpretError {
    pub code: InterpretErrorCode,
    pub provenance: Provenance,
    pub message: String,
}

impl InterpretError {
    /// Convenience accessor for the innermost source span.
    pub fn span(&self) -> Span {
        self.provenance.site
    }

    /// Construct an error with an explicit [`InterpretErrorCode`].
    pub fn new(
        code: InterpretErrorCode,
        provenance: Provenance,
        message: impl Into<String>,
    ) -> Self {
        Self { code, provenance, message: message.into() }
    }
}

impl std::fmt::Display for InterpretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.provenance.site;
        write!(f, "{} (at {}..{})", self.message, s.start, s.end)
    }
}

impl std::error::Error for InterpretError {}

/// Unified error returned by the [`build`] / [`build_with_base_dir`]
/// convenience path — carries either a descriptor-level [`BindError`]
/// that short-circuited the bind stage, or a runtime [`InterpretError`]
/// from graph construction. Fail-fast consumers that want to surface
/// every bind error for a user should drive
/// [`descriptor_bind::bind_with_base_dir`] + [`build_from_bound`]
/// themselves; this wrapper exists for callers that prefer a single
/// `?`-chainable entry point.
#[derive(Debug)]
pub struct BuildError {
    pub message: String,
    pub provenance: Provenance,
    pub source: BuildErrorSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildErrorSource {
    Bind(BindErrorCode),
    Interpret(InterpretErrorCode),
}

impl BuildError {
    pub fn span(&self) -> Span {
        self.provenance.site
    }

    pub fn code(&self) -> &'static str {
        match self.source {
            BuildErrorSource::Bind(c) => c.as_str(),
            BuildErrorSource::Interpret(c) => c.as_str(),
        }
    }

    pub fn from_bind(err: &BindError) -> Self {
        Self {
            message: err.message.clone(),
            provenance: err.provenance.clone(),
            source: BuildErrorSource::Bind(err.code),
        }
    }

    pub fn from_interpret(err: InterpretError) -> Self {
        Self {
            message: err.message,
            provenance: err.provenance,
            source: BuildErrorSource::Interpret(err.code),
        }
    }
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.provenance.site;
        write!(f, "{} (at {}..{})", self.message, s.start, s.end)
    }
}

impl std::error::Error for BuildError {}

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
    build_from_bound(flat, &bound, env).map_err(BuildError::from_interpret)
}

/// Build a [`ModuleGraph`] (and optional [`TrackerData`]) from a
/// [`FlatPatch`] together with its [`BoundPatch`] (produced by
/// [`descriptor_bind::bind_with_base_dir`]).
///
/// The caller is responsible for having checked [`BoundPatch::errors`];
/// unresolved modules are skipped — if a referenced module is missing a
/// descriptor, this function returns an [`InterpretError::Other`]
/// rather than swallowing the violation. `flat` is still consulted for
/// pattern and song definitions, which live outside the bound-graph
/// artifact.
/// Defensive guard used by [`build_from_bound`] to pattern-match a bound
/// item's `Resolved` variant.
///
/// **Invariant — callers must have checked [`BoundPatch::errors`] before
/// invoking [`build_from_bound`].** If this guard fires in production the
/// pipeline layering has been violated (the short-circuit above
/// `build_from_bound` was skipped): the error here is deliberately
/// [`InterpretErrorCode::Other`] rather than a user-facing code.
fn require_resolved<'a, I: BoundItem<'a>>(
    item: &'a I,
    stage: &str,
) -> Result<&'a I::ResolvedTy, InterpretError> {
    item.resolved().ok_or_else(|| {
        InterpretError::new(
            InterpretErrorCode::Other,
            item.provenance().clone(),
            format!(
                "unresolved {stage} reached build; bind errors must be handled before build"
            ),
        )
    })
}

/// Minimal accessor trait for bound items so [`require_resolved`] can
/// discharge the three defensive checks uniformly.
trait BoundItem<'a> {
    type ResolvedTy;
    fn resolved(&'a self) -> Option<&'a Self::ResolvedTy>;
    fn provenance(&self) -> &Provenance;
}

impl<'a> BoundItem<'a> for BoundModule {
    type ResolvedTy = ResolvedModule;
    fn resolved(&'a self) -> Option<&'a Self::ResolvedTy> {
        self.as_resolved()
    }
    fn provenance(&self) -> &Provenance {
        BoundModule::provenance(self)
    }
}

impl<'a> BoundItem<'a> for BoundConnection {
    type ResolvedTy = ResolvedConnection;
    fn resolved(&'a self) -> Option<&'a Self::ResolvedTy> {
        match self {
            BoundConnection::Resolved(r) => Some(r),
            BoundConnection::Unresolved(_) => None,
        }
    }
    fn provenance(&self) -> &Provenance {
        BoundConnection::provenance(self)
    }
}

impl<'a> BoundItem<'a> for BoundPortRef {
    type ResolvedTy = ResolvedPortRef;
    fn resolved(&'a self) -> Option<&'a Self::ResolvedTy> {
        match self {
            BoundPortRef::Resolved(r) => Some(r),
            BoundPortRef::Unresolved(_) => None,
        }
    }
    fn provenance(&self) -> &Provenance {
        BoundPortRef::provenance(self)
    }
}

pub fn build_from_bound(
    flat: &FlatPatch,
    bound: &BoundPatch,
    _env: &AudioEnvironment,
) -> Result<BuildResult, InterpretError> {
    let mut graph = ModuleGraph::new();

    // Pre-compute the song name-to-index map — still needed by tracker
    // data assembly and MasterSequencer song-reference validation.
    let song_name_to_index: HashMap<String, usize> = {
        let mut names: Vec<String> = flat.songs.iter().map(|s| s.name.to_string()).collect();
        names.sort();
        names.into_iter().enumerate().map(|(i, n)| (n, i)).collect()
    };

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
    let tracker_data = build_tracker_data(flat, &graph, &song_name_to_index)?;

    Ok(BuildResult { graph, tracker_data })
}

// ── Tracker data construction ────────────────────────────────────────────────

/// Build [`TrackerData`] from the pattern and song definitions in a [`FlatPatch`].
///
/// Returns `None` if there are no patterns and no songs.
fn build_tracker_data(
    flat: &FlatPatch,
    graph: &ModuleGraph,
    song_name_to_index: &HashMap<String, usize>,
) -> Result<Option<TrackerData>, InterpretError> {
    if flat.patterns.is_empty() && flat.songs.is_empty() {
        return Ok(None);
    }

    // Patterns Vec order follows `flat.patterns` positional order; expansion's
    // `FlatSongRow` indices refer directly to this list.
    let mut patterns: Vec<Pattern> = Vec::with_capacity(flat.patterns.len());
    for fp in &flat.patterns {
        let max_steps = fp.channels.iter().map(|c| c.steps.len()).max().unwrap_or(0);
        let mut data = Vec::with_capacity(fp.channels.len());
        for ch in &fp.channels {
            let mut steps = Vec::with_capacity(max_steps);
            for s in &ch.steps {
                steps.push(convert_step(s));
            }
            // Pad shorter channels with rest steps.
            while steps.len() < max_steps {
                steps.push(TrackerStep {
                    cv1: 0.0, cv2: 0.0,
                    trigger: false, gate: false,
                    cv1_end: None, cv2_end: None,
                    repeat: 1,
                });
            }
            data.push(steps);
        }
        patterns.push(Pattern {
            channels: fp.channels.len(),
            steps: max_steps,
            data,
        });
    }

    let pattern_display_name = |idx: usize| -> &str {
        flat.patterns
            .get(idx)
            .map(|p| p.name.name.as_str())
            .unwrap_or("?")
    };

    // Convert DSL songs to runtime Songs (alphabetical order so that Vec
    // indices match the pre-computed song_name_to_index map in the caller).
    let mut sorted_song_defs: Vec<&_> = flat.songs.iter().collect();
    sorted_song_defs.sort_by(|a, b| a.name.cmp(&b.name));
    let mut song_list: Vec<Song> = Vec::new();
    for song_def in &sorted_song_defs {
        // Validate: patterns within a single song column must have the same
        // step count and channel count. (Pattern existence is enforced in the
        // expansion stage, so every `Some(idx)` is guaranteed to be in range.)
        for col_idx in 0..song_def.channels.len() {
            let col_name = &song_def.channels[col_idx].name;
            let mut col_step_count: Option<(usize, &str)> = None;
            let mut col_chan_count: Option<(usize, &str)> = None;
            for row in &song_def.rows {
                if let Some(Some(idx)) = row.cells.get(col_idx) {
                    let pat = &patterns[*idx];
                    let pat_name = pattern_display_name(*idx);
                    if let Some((expected_steps, first_name)) = col_step_count {
                        if pat.steps != expected_steps {
                            return Err(InterpretError::new(InterpretErrorCode::TrackerShape, song_def.provenance.clone(), format!(
                                    "song '{}' channel '{}': pattern '{}' has {} steps but '{}' has {}",
                                    song_def.name, col_name,
                                    pat_name, pat.steps,
                                    first_name, expected_steps,
                                )));
                        }
                    } else {
                        col_step_count = Some((pat.steps, pat_name));
                    }
                    if let Some((expected_chans, first_name)) = col_chan_count {
                        if pat.channels != expected_chans {
                            return Err(InterpretError::new(InterpretErrorCode::SequencerSongMismatch, song_def.provenance.clone(), format!(
                                    "song '{}' channel '{}': pattern '{}' has {} channels but '{}' has {}",
                                    song_def.name, col_name,
                                    pat_name, pat.channels,
                                    first_name, expected_chans,
                                )));
                        }
                    } else {
                        col_chan_count = Some((pat.channels, pat_name));
                    }
                }
            }
        }

        let order: Vec<Vec<Option<usize>>> = song_def
            .rows
            .iter()
            .map(|row| row.cells.clone())
            .collect();

        let song = Song {
            channels: song_def.channels.len(),
            order,
            loop_point: song_def.loop_point.unwrap_or(0),
        };
        song_list.push(song);
    }

    let song_bank = SongBank { songs: song_list };

    // Validate: MasterSequencer song references and channel matching.
    // `song_name_to_index` is the shared map computed once by the caller.
    validate_sequencer_songs(graph, &song_bank, song_name_to_index, flat)?;

    Ok(Some(TrackerData {
        patterns: PatternBank { patterns },
        songs: song_bank,
    }))
}

/// Convert a DSL [`patches_dsl::ast::Step`] to a runtime [`TrackerStep`].
fn convert_step(dsl_step: &patches_dsl::ast::Step) -> TrackerStep {
    TrackerStep {
        cv1: dsl_step.cv1,
        cv2: dsl_step.cv2,
        trigger: dsl_step.trigger,
        gate: dsl_step.gate,
        cv1_end: dsl_step.cv1_end,
        cv2_end: dsl_step.cv2_end,
        repeat: dsl_step.repeat,
    }
}

/// Validate that every MasterSequencer's `song` parameter references a defined
/// song, and that the song's column headers match the sequencer's channels.
fn validate_sequencer_songs(
    _graph: &ModuleGraph,
    song_bank: &SongBank,
    song_name_to_index: &HashMap<String, usize>,
    flat: &FlatPatch,
) -> Result<(), InterpretError> {
    for flat_module in &flat.modules {
        if flat_module.type_name != "MasterSequencer" {
            continue;
        }
        // Find the `song` parameter value.
        let song_name = flat_module.params.iter().find_map(|(name, value)| {
            if name == "song" {
                match value {
                    Value::Scalar(Scalar::Str(s)) => Some(s.as_str()),
                    _ => None,
                }
            } else {
                None
            }
        });
        if let Some(song_name) = song_name {
            let Some(&song_idx) = song_name_to_index.get(song_name) else {
                return Err(InterpretError::new(InterpretErrorCode::SequencerSongMismatch, flat_module.provenance.clone(), format!(
                        "MasterSequencer '{}': song '{}' not found",
                        flat_module.id, song_name,
                    )));
            };
            // Validate channel matching: the song's column count must match
            // the sequencer's declared channel count.
            let song = &song_bank.songs[song_idx];
            let seq_channels = flat_module.shape.iter().find_map(|(name, scalar)| {
                if name == "channels" {
                    if let Scalar::Int(n) = scalar { Some(*n as usize) } else { None }
                } else {
                    None
                }
            }).unwrap_or(0);
            if seq_channels != song.channels {
                return Err(InterpretError::new(InterpretErrorCode::SequencerSongMismatch, flat_module.provenance.clone(), format!(
                        "MasterSequencer '{}': has {} channels but song '{}' has {} columns",
                        flat_module.id, seq_channels, song_name, song.channels,
                    )));
            }
        }
    }
    Ok(())
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

/// Convert a DSL [`Value`] to a [`patches_core::ParameterValue`] given the
/// expected [`patches_core::ParameterKind`] from the module descriptor.
fn convert_value(
    value: &Value,
    kind: &patches_core::ParameterKind,
    song_name_to_index: &HashMap<String, usize>,
) -> Result<patches_core::ParameterValue, ParamConversionError> {
    use patches_core::{ParameterKind, ParameterValue};
    match (value, kind) {
        (Value::Scalar(Scalar::Float(f)), ParameterKind::Float { .. }) => {
            Ok(ParameterValue::Float(*f as f32))
        }
        (Value::Scalar(Scalar::Int(i)), ParameterKind::Float { .. }) => {
            Ok(ParameterValue::Float(*i as f32))
        }
        (Value::Scalar(Scalar::Int(i)), ParameterKind::Int { .. }) => {
            Ok(ParameterValue::Int(*i))
        }
        (Value::Scalar(Scalar::Bool(b)), ParameterKind::Bool { .. }) => {
            Ok(ParameterValue::Bool(*b))
        }
        (Value::Scalar(Scalar::Str(s)), ParameterKind::Enum { variants, .. }) => variants
            .iter()
            .find(|&&v| v == s.as_str())
            .map(|&v| ParameterValue::Enum(v))
            .ok_or_else(|| {
                ParamConversionError::OutOfRange(format!("invalid enum variant '{s}'"))
            }),
        (Value::Scalar(Scalar::Str(s)), ParameterKind::String { .. }) => {
            Ok(ParameterValue::String(s.clone()))
        }
        (Value::File(path), ParameterKind::File { extensions }) => {
            if !path.is_empty() {
                let ext = std::path::Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if !extensions.is_empty() && !extensions.iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
                    return Err(ParamConversionError::OutOfRange(format!(
                        "unsupported file extension '.{ext}'; expected one of: {}",
                        extensions.join(", ")
                    )));
                }
            }
            Ok(ParameterValue::File(path.clone()))
        }
        (Value::Scalar(Scalar::Str(s)), ParameterKind::SongName) => {
            if s.is_empty() {
                Ok(ParameterValue::Int(-1))
            } else {
                song_name_to_index
                    .get(s.as_str())
                    .map(|&idx| ParameterValue::Int(idx as i64))
                    .ok_or_else(|| {
                        ParamConversionError::OutOfRange(format!("song '{s}' not found"))
                    })
            }
        }
        _ => Err(ParamConversionError::TypeMismatch(format!(
            "expected {}, found {}",
            kind.kind_name(),
            value_kind_name(value)
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
