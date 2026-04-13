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

use std::collections::HashMap;
use std::path::Path;

use patches_core::{
    AudioEnvironment, ModuleGraph, ModuleShape, Registry,
    ParameterMap, ParameterValue, ParameterKind,
    PortRef,
    TrackerData, PatternBank, SongBank, Pattern, Song, TrackerStep,
};
use patches_dsl::ast::{Scalar, SongCell, Span, Value};
use patches_dsl::flat::{FlatConnection, FlatModule, FlatPatch};

/// An error produced during interpretation of a [`FlatPatch`].
///
/// Carries the source [`Span`] of the offending construct and a
/// human-readable message describing the problem.
#[derive(Debug)]
pub struct InterpretError {
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for InterpretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at {}..{})", self.message, self.span.start, self.span.end)
    }
}

impl std::error::Error for InterpretError {}

/// The result of interpreting a [`FlatPatch`]: a module graph and optional
/// tracker data (patterns and songs).
pub struct BuildResult {
    pub graph: ModuleGraph,
    pub tracker_data: Option<TrackerData>,
}

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
    _env: &AudioEnvironment,
) -> Result<BuildResult, InterpretError> {
    build_with_base_dir(flat, registry, _env, None)
}

/// Build a [`ModuleGraph`] (and optional [`TrackerData`]) from a validated
/// [`FlatPatch`], resolving relative file paths against `base_dir`.
///
/// String parameters whose descriptor name is `"path"` are resolved
/// relative to `base_dir` when the value is not already an absolute path.
/// Pass `None` to leave paths as-is (same behaviour as [`build`]).
pub fn build_with_base_dir(
    flat: &FlatPatch,
    registry: &Registry,
    _env: &AudioEnvironment,
    base_dir: Option<&Path>,
) -> Result<BuildResult, InterpretError> {
    let mut graph = ModuleGraph::new();

    // Pre-compute the song name-to-index map (alphabetical order, matching
    // the Vec order used when building SongBank in stage 3). This is needed
    // in stage 1 so that SongName parameters can be resolved to indices.
    let song_name_to_index: HashMap<String, usize> = {
        let mut names: Vec<&str> = flat.songs.iter().map(|s| s.name.name.as_str()).collect();
        names.sort();
        names.iter().enumerate().map(|(i, &n)| (n.to_string(), i)).collect()
    };

    // Stage 1 — add all module nodes.
    for flat_module in &flat.modules {
        add_module(&mut graph, flat_module, registry, base_dir, &song_name_to_index)?;
    }

    // Stage 2 — add all connections (after all nodes are present so that
    // forward references within a patch are not errors).
    for conn in &flat.connections {
        add_connection(&mut graph, conn)?;
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

    // Build the name → bank index mapping (alphabetical sort on pattern names).
    let mut sorted_names: Vec<&str> = flat.patterns.iter().map(|p| p.name.as_str()).collect();
    sorted_names.sort();
    let name_to_index: HashMap<&str, usize> = sorted_names
        .iter()
        .enumerate()
        .map(|(i, &name)| (name, i))
        .collect();

    // Convert DSL patterns to runtime Patterns.
    let mut indexed_patterns: Vec<Option<Pattern>> = vec![None; flat.patterns.len()];
    for fp in &flat.patterns {
        let bank_idx = name_to_index[fp.name.as_str()];
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
        indexed_patterns[bank_idx] = Some(Pattern {
            channels: fp.channels.len(),
            steps: max_steps,
            data,
        });
    }
    let patterns: Vec<Pattern> = indexed_patterns.into_iter().flatten().collect();

    // Convert DSL songs to runtime Songs (alphabetical order so that Vec
    // indices match the pre-computed song_name_to_index map in the caller).
    let mut sorted_song_defs: Vec<&_> = flat.songs.iter().collect();
    sorted_song_defs.sort_by_key(|s| &s.name.name);
    let mut song_list: Vec<Song> = Vec::new();
    for song_def in &sorted_song_defs {
        // Validate: every pattern name in the song must exist.
        for (row_idx, row) in song_def.rows.iter().enumerate() {
            for (col_idx, cell) in row.cells.iter().enumerate() {
                if let SongCell::Pattern(ref pat_name) = cell {
                    if !name_to_index.contains_key(pat_name.name.as_str()) {
                        return Err(InterpretError {
                            span: song_def.span,
                            message: format!(
                                "song '{}' row {} channel '{}': pattern '{}' not found",
                                song_def.name.name,
                                row_idx + 1,
                                song_def.channels.get(col_idx).map_or("?", |c| &c.name),
                                pat_name.name,
                            ),
                        });
                    }
                }
            }
        }

        // Validate: patterns within a single song column must have the same
        // step count and channel count.
        for col_idx in 0..song_def.channels.len() {
            let col_name = &song_def.channels[col_idx].name;
            let mut col_step_count: Option<(usize, &str)> = None;
            let mut col_chan_count: Option<(usize, &str)> = None;
            for row in &song_def.rows {
                if let Some(SongCell::Pattern(ref pat_name)) = row.cells.get(col_idx) {
                    let bank_idx = name_to_index[pat_name.name.as_str()];
                    let pat = &patterns[bank_idx];
                    if let Some((expected_steps, first_name)) = col_step_count {
                        if pat.steps != expected_steps {
                            return Err(InterpretError {
                                span: song_def.span,
                                message: format!(
                                    "song '{}' channel '{}': pattern '{}' has {} steps but '{}' has {}",
                                    song_def.name.name, col_name,
                                    pat_name.name, pat.steps,
                                    first_name, expected_steps,
                                ),
                            });
                        }
                    } else {
                        col_step_count = Some((pat.steps, &pat_name.name));
                    }
                    if let Some((expected_chans, first_name)) = col_chan_count {
                        if pat.channels != expected_chans {
                            return Err(InterpretError {
                                span: song_def.span,
                                message: format!(
                                    "song '{}' channel '{}': pattern '{}' has {} channels but '{}' has {}",
                                    song_def.name.name, col_name,
                                    pat_name.name, pat.channels,
                                    first_name, expected_chans,
                                ),
                            });
                        }
                    } else {
                        col_chan_count = Some((pat.channels, &pat_name.name));
                    }
                }
            }
        }

        let order: Vec<Vec<Option<usize>>> = song_def.rows.iter().map(|row| {
            row.cells.iter().map(|cell| {
                match cell {
                    SongCell::Pattern(pat_name) => Some(name_to_index[pat_name.name.as_str()]),
                    _ => None,
                }
            }).collect()
        }).collect();

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
                return Err(InterpretError {
                    span: flat_module.span,
                    message: format!(
                        "MasterSequencer '{}': song '{}' not found",
                        flat_module.id, song_name,
                    ),
                });
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
                return Err(InterpretError {
                    span: flat_module.span,
                    message: format!(
                        "MasterSequencer '{}': has {} channels but song '{}' has {} columns",
                        flat_module.id, seq_channels, song_name, song.channels,
                    ),
                });
            }
        }
    }
    Ok(())
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn add_module(
    graph: &mut ModuleGraph,
    flat_module: &FlatModule,
    registry: &Registry,
    base_dir: Option<&Path>,
    song_name_to_index: &HashMap<String, usize>,
) -> Result<(), InterpretError> {
    let shape = shape_from_args(&flat_module.shape);

    let descriptor = registry
        .describe(&flat_module.type_name, &shape)
        .map_err(|e| InterpretError {
            span: flat_module.span,
            message: e.to_string(),
        })?;

    let params = convert_params(&flat_module.params, &descriptor, base_dir, song_name_to_index)
        .map_err(|msg| {
            InterpretError { span: flat_module.span, message: msg }
        })?;

    patches_core::validate_parameters(&params, &descriptor).map_err(|e| InterpretError {
        span: flat_module.span,
        message: e.to_string(),
    })?;

    graph
        .add_module(flat_module.id.clone(), descriptor, &params)
        .map_err(|e| InterpretError {
            span: flat_module.span,
            message: e.to_string(),
        })
}

fn add_connection(
    graph: &mut ModuleGraph,
    conn: &FlatConnection,
) -> Result<(), InterpretError> {
    let from_id = patches_core::NodeId::from(conn.from_module.clone());
    let to_id = patches_core::NodeId::from(conn.to_module.clone());

    // Resolve source output port — borrow and copy the &'static str name from
    // the descriptor so we can call connect() without holding the borrow.
    let output_port = {
        let from_node = graph.get_node(&from_id).ok_or_else(|| InterpretError {
            span: conn.span,
            message: format!("module '{}' not found", conn.from_module),
        })?;
        from_node
            .module_descriptor
            .outputs
            .iter()
            .find(|p| p.name == conn.from_port.as_str() && p.index == conn.from_index as usize)
            .map(|p| PortRef { name: p.name, index: p.index })
            .ok_or_else(|| {
                let available: Vec<String> = from_node.module_descriptor.outputs.iter()
                    .map(|p| format!("{}/{}", p.name, p.index))
                    .collect();
                InterpretError {
                    span: conn.span,
                    message: format!(
                        "module '{}' has no output port '{}/{}'; available outputs: [{}]",
                        conn.from_module, conn.from_port, conn.from_index, available.join(", ")
                    ),
                }
            })?
    };

    // Resolve destination input port.
    let input_port = {
        let to_node = graph.get_node(&to_id).ok_or_else(|| InterpretError {
            span: conn.span,
            message: format!("module '{}' not found", conn.to_module),
        })?;
        to_node
            .module_descriptor
            .inputs
            .iter()
            .find(|p| p.name == conn.to_port.as_str() && p.index == conn.to_index as usize)
            .map(|p| PortRef { name: p.name, index: p.index })
            .ok_or_else(|| {
                let available: Vec<String> = to_node.module_descriptor.inputs.iter()
                    .map(|p| format!("{}/{}", p.name, p.index))
                    .collect();
                InterpretError {
                    span: conn.span,
                    message: format!(
                        "module '{}' has no input port '{}/{}'; available inputs: [{}]",
                        conn.to_module, conn.to_port, conn.to_index, available.join(", ")
                    ),
                }
            })?
    };

    graph
        .connect(&from_id, output_port, &to_id, input_port, conn.scale as f32)
        .map_err(|e| InterpretError {
            span: conn.span,
            message: e.to_string(),
        })
}

/// Convert `Vec<(String, Scalar)>` shape arguments to a [`ModuleShape`].
///
/// Recognised keys are `"channels"` and `"length"`; unrecognised keys are
/// silently ignored (the registry's `describe` implementation is responsible
/// for validating shape semantics).
fn shape_from_args(args: &[(String, Scalar)]) -> ModuleShape {
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
    ModuleShape { channels, length, high_quality }
}

/// Parse a parameter name string of the form `"name"` or `"name/N"` into a
/// base name and index.
fn parse_param_name(name: &str) -> (&str, usize) {
    if let Some(pos) = name.rfind('/') {
        let base = &name[..pos];
        let idx_str = &name[pos + 1..];
        if let Ok(idx) = idx_str.parse::<usize>() {
            return (base, idx);
        }
    }
    (name, 0)
}

/// Convert a slice of `(name, Value)` DSL param pairs into a [`ParameterMap`],
/// validating each value's type against the module's [`patches_core::ModuleDescriptor`].
///
/// Returns `Err(message)` on the first type incompatibility or unrecognised
/// parameter name encountered.
fn convert_params(
    params: &[(String, Value)],
    descriptor: &patches_core::ModuleDescriptor,
    base_dir: Option<&Path>,
    song_name_to_index: &HashMap<String, usize>,
) -> Result<ParameterMap, String> {
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
                format!(
                    "unknown parameter '{raw_name}'; known parameters: {}",
                    known.join(", ")
                )
            })?;

        let mut pv = convert_value(value, &param_desc.parameter_type, song_name_to_index)
            .map_err(|e| format!("parameter '{raw_name}': {e}"))?;

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

/// Convert a DSL [`Value`] to a [`ParameterValue`] given the expected
/// [`ParameterKind`] from the module descriptor.
///
/// Integer literals are accepted where a float is expected (widening
/// conversion). Enum string values are resolved to a `&'static str` from the
/// declared variant list.
fn convert_value(
    value: &Value,
    kind: &ParameterKind,
    song_name_to_index: &HashMap<String, usize>,
) -> Result<ParameterValue, String> {
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
            .ok_or_else(|| format!("invalid enum variant '{s}'")),
        (Value::Scalar(Scalar::Str(s)), ParameterKind::String { .. }) => {
            Ok(ParameterValue::String(s.clone()))
        }
        (Value::File(path), ParameterKind::File { extensions }) => {
            // Validate file extension if the path is non-empty.
            if !path.is_empty() {
                let ext = std::path::Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if !extensions.is_empty() && !extensions.iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
                    return Err(format!(
                        "unsupported file extension '.{ext}'; expected one of: {}",
                        extensions.join(", ")
                    ));
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
                    .ok_or_else(|| format!("song '{s}' not found"))
            }
        }
        (Value::Array(items), ParameterKind::Array { .. }) => {
            let strings = items
                .iter()
                .map(|item| match item {
                    Value::Scalar(Scalar::Str(s)) => Ok(s.clone()),
                    _ => Err("array elements must be strings".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ParameterValue::Array(strings.into()))
        }
        _ => Err(format!("expected {}, found {}", kind.kind_name(), value_kind_name(value))),
    }
}

fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Scalar(Scalar::Float(_)) => "float",
        Value::Scalar(Scalar::Int(_)) => "int",
        Value::Scalar(Scalar::Bool(_)) => "bool",
        Value::Scalar(Scalar::Str(_)) => "string",
        Value::Scalar(Scalar::ParamRef(_)) => "param-ref",
        Value::Array(_) => "array",
        Value::Table(_) => "table",
        Value::File(_) => "file",
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_dsl::flat::{FlatConnection, FlatModule, FlatPatch, FlatPatternDef, FlatPatternChannel};
    use patches_dsl::ast::{Ident, Scalar, SongCell, SongDef, SongRow, Span, Step, Value};

    fn span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn env() -> AudioEnvironment {
        AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
    }

    fn registry() -> Registry {
        patches_modules::default_registry()
    }

    fn osc_module(id: &str) -> FlatModule {
        FlatModule {
            id: id.to_string(),
            type_name: "Osc".to_string(),
            shape: vec![],
            params: vec![],
            span: span(),
        }
    }

    fn sum_module(id: &str, channels: i64) -> FlatModule {
        FlatModule {
            id: id.to_string(),
            type_name: "Sum".to_string(),
            shape: vec![("channels".to_string(), Scalar::Int(channels))],
            params: vec![],
            span: span(),
        }
    }

    fn connection(
        from_module: &str, from_port: &str, from_index: u32,
        to_module: &str, to_port: &str, to_index: u32,
    ) -> FlatConnection {
        FlatConnection {
            from_module: from_module.to_string(),
            from_port: from_port.to_string(),
            from_index,
            to_module: to_module.to_string(),
            to_port: to_port.to_string(),
            to_index,
            scale: 1.0,
            span: span(),
        }
    }

    fn empty_flat() -> FlatPatch {
        FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![],
            connections: vec![],
        }
    }

    fn trigger_step() -> Step {
        Step { cv1: 0.0, cv2: 0.0, trigger: true, gate: true, cv1_end: None, cv2_end: None, repeat: 1 }
    }

    fn rest_step() -> Step {
        Step { cv1: 0.0, cv2: 0.0, trigger: false, gate: false, cv1_end: None, cv2_end: None, repeat: 1 }
    }

    fn ident(name: &str) -> Ident {
        Ident { name: name.to_string(), span: span() }
    }

    // ── Existing module/connection tests ─────────────────────────────────

    #[test]
    fn build_single_module_patch() {
        let mut flat = empty_flat();
        flat.modules = vec![osc_module("osc1")];
        let result = build(&flat, &registry(), &env()).unwrap();
        assert_eq!(result.graph.node_ids().len(), 1);
    }

    #[test]
    fn build_two_modules_with_connection() {
        let mut flat = empty_flat();
        flat.modules = vec![osc_module("osc1"), sum_module("mix", 1)];
        flat.connections = vec![connection("osc1", "sine", 0, "mix", "in", 0)];
        let result = build(&flat, &registry(), &env()).unwrap();
        assert_eq!(result.graph.node_ids().len(), 2);
        assert_eq!(result.graph.edge_list().len(), 1);
    }

    #[test]
    fn forward_references_are_not_errors() {
        let mut flat = empty_flat();
        flat.modules = vec![sum_module("mix", 1), osc_module("osc1")];
        flat.connections = vec![connection("osc1", "sine", 0, "mix", "in", 0)];
        assert!(build(&flat, &registry(), &env()).is_ok());
    }

    #[test]
    fn unknown_type_name_returns_interpret_error() {
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "x".to_string(),
            type_name: "NonExistentModule".to_string(),
            shape: vec![],
            params: vec![],
            span: Span { start: 10, end: 20 },
        }];
        let err = build(&flat, &registry(), &env()).unwrap_err();
        assert!(err.message.contains("NonExistentModule"));
        assert_eq!(err.span, Span { start: 10, end: 20 });
    }

    #[test]
    fn unknown_output_port_returns_interpret_error() {
        let mut flat = empty_flat();
        flat.modules = vec![osc_module("osc1"), sum_module("mix", 1)];
        flat.connections = vec![FlatConnection {
            from_module: "osc1".to_string(),
            from_port: "no_such_out".to_string(),
            from_index: 0,
            to_module: "mix".to_string(),
            to_port: "in".to_string(),
            to_index: 0,
            scale: 1.0,
            span: Span { start: 5, end: 15 },
        }];
        let err = build(&flat, &registry(), &env()).unwrap_err();
        assert!(err.message.contains("no_such_out"));
        assert_eq!(err.span, Span { start: 5, end: 15 });
    }

    #[test]
    fn unknown_input_port_returns_interpret_error() {
        let mut flat = empty_flat();
        flat.modules = vec![osc_module("osc1"), sum_module("mix", 1)];
        flat.connections = vec![FlatConnection {
            from_module: "osc1".to_string(),
            from_port: "sine".to_string(),
            from_index: 0,
            to_module: "mix".to_string(),
            to_port: "no_such_in".to_string(),
            to_index: 0,
            scale: 1.0,
            span: Span { start: 3, end: 9 },
        }];
        let err = build(&flat, &registry(), &env()).unwrap_err();
        assert!(err.message.contains("no_such_in"));
        assert_eq!(err.span, Span { start: 3, end: 9 });
    }

    #[test]
    fn graph_error_wrapped_with_span() {
        let osc2 = FlatModule {
            id: "osc2".to_string(),
            type_name: "Osc".to_string(),
            shape: vec![],
            params: vec![],
            span: span(),
        };
        let dup_conn = FlatConnection {
            from_module: "osc2".to_string(),
            from_port: "sine".to_string(),
            from_index: 0,
            to_module: "mix".to_string(),
            to_port: "in".to_string(),
            to_index: 0,
            scale: 1.0,
            span: Span { start: 50, end: 60 },
        };
        let mut flat = empty_flat();
        flat.modules = vec![osc_module("osc1"), osc2, sum_module("mix", 1)];
        flat.connections = vec![
            connection("osc1", "sine", 0, "mix", "in", 0),
            dup_conn,
        ];
        let err = build(&flat, &registry(), &env()).unwrap_err();
        assert_eq!(err.span, Span { start: 50, end: 60 });
    }

    #[test]
    fn float_param_is_accepted() {
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "osc1".to_string(),
            type_name: "Osc".to_string(),
            shape: vec![],
            params: vec![
                ("frequency".to_string(), Value::Scalar(Scalar::Float((440.0_f64 / 16.351_597_831_287_414).log2()))),
            ],
            span: span(),
        }];
        assert!(build(&flat, &registry(), &env()).is_ok());
    }

    #[test]
    fn enum_param_is_accepted() {
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "osc1".to_string(),
            type_name: "Osc".to_string(),
            shape: vec![],
            params: vec![
                ("fm_type".to_string(), Value::Scalar(Scalar::Str("logarithmic".to_string()))),
            ],
            span: span(),
        }];
        assert!(build(&flat, &registry(), &env()).is_ok());
    }

    #[test]
    fn poly_synth_layered_patches_file_builds() {
        let src = std::fs::read_to_string(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/poly_synth_layered.patches"),
        )
        .expect("poly_synth_layered.patches not found");
        let file = patches_dsl::parse(&src).expect("parse failed");
        let result = patches_dsl::expand(&file).expect("expand failed");
        let build_result = build(&result.patch, &registry(), &env()).expect("build failed");
        assert_eq!(build_result.graph.node_ids().len(), 27);
    }

    #[test]
    fn poly_synth_patches_file_builds() {
        let src = std::fs::read_to_string(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/poly_synth.patches"),
        )
        .expect("poly_synth.patches not found");
        let file = patches_dsl::parse(&src).expect("parse failed");
        let result = patches_dsl::expand(&file).expect("expand failed");
        let build_result = build(&result.patch, &registry(), &env()).expect("build failed");
        assert_eq!(build_result.graph.node_ids().len(), 11);
        assert_eq!(build_result.graph.edge_list().len(), 16);
    }

    #[test]
    fn unknown_param_name_returns_interpret_error() {
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "osc1".to_string(),
            type_name: "Osc".to_string(),
            shape: vec![],
            params: vec![
                ("no_such_param".to_string(), Value::Scalar(Scalar::Float(1.0))),
            ],
            span: Span { start: 1, end: 5 },
        }];
        let err = build(&flat, &registry(), &env()).unwrap_err();
        assert!(err.message.contains("no_such_param"));
    }

    // ── Tracker data tests ──────────────────────────────────────────────

    #[test]
    fn no_patterns_or_songs_returns_none() {
        let result = build(&empty_flat(), &registry(), &env()).unwrap();
        assert!(result.tracker_data.is_none());
    }

    #[test]
    fn single_pattern_builds_tracker_data() {
        let mut flat = empty_flat();
        flat.patterns = vec![FlatPatternDef {
            name: "drums".to_string(),
            channels: vec![
                FlatPatternChannel {
                    name: "kick".to_string(),
                    steps: vec![trigger_step(), rest_step(), rest_step(), rest_step()],
                },
                FlatPatternChannel {
                    name: "snare".to_string(),
                    steps: vec![rest_step(), rest_step(), trigger_step(), rest_step()],
                },
            ],
            span: span(),
        }];
        let result = build(&flat, &registry(), &env()).unwrap();
        let td = result.tracker_data.unwrap();
        assert_eq!(td.patterns.patterns.len(), 1);
        let pat = &td.patterns.patterns[0];
        assert_eq!(pat.channels, 2);
        assert_eq!(pat.steps, 4);
        assert!(pat.data[0][0].trigger); // kick step 0
        assert!(!pat.data[0][1].trigger); // kick step 1
        assert!(!pat.data[1][0].trigger); // snare step 0
        assert!(pat.data[1][2].trigger); // snare step 2
    }

    #[test]
    fn pattern_bank_indices_are_alphabetical() {
        let mut flat = empty_flat();
        // Add patterns in non-alphabetical order.
        flat.patterns = vec![
            FlatPatternDef {
                name: "charlie".to_string(),
                channels: vec![FlatPatternChannel {
                    name: "ch".to_string(),
                    steps: vec![trigger_step()],
                }],
                span: span(),
            },
            FlatPatternDef {
                name: "alpha".to_string(),
                channels: vec![FlatPatternChannel {
                    name: "ch".to_string(),
                    steps: vec![rest_step()],
                }],
                span: span(),
            },
            FlatPatternDef {
                name: "bravo".to_string(),
                channels: vec![FlatPatternChannel {
                    name: "ch".to_string(),
                    steps: vec![trigger_step(), rest_step()],
                }],
                span: span(),
            },
        ];
        let result = build(&flat, &registry(), &env()).unwrap();
        let td = result.tracker_data.unwrap();
        // alpha=0, bravo=1, charlie=2
        assert_eq!(td.patterns.patterns[0].steps, 1); // alpha: 1 step (rest)
        assert!(!td.patterns.patterns[0].data[0][0].trigger);
        assert_eq!(td.patterns.patterns[1].steps, 2); // bravo: 2 steps
        assert_eq!(td.patterns.patterns[2].steps, 1); // charlie: 1 step (trigger)
        assert!(td.patterns.patterns[2].data[0][0].trigger);
    }

    #[test]
    fn song_resolves_pattern_references() {
        let mut flat = empty_flat();
        flat.patterns = vec![
            FlatPatternDef {
                name: "pat_a".to_string(),
                channels: vec![FlatPatternChannel {
                    name: "ch".to_string(),
                    steps: vec![trigger_step()],
                }],
                span: span(),
            },
            FlatPatternDef {
                name: "pat_b".to_string(),
                channels: vec![FlatPatternChannel {
                    name: "ch".to_string(),
                    steps: vec![rest_step()],
                }],
                span: span(),
            },
        ];
        flat.songs = vec![SongDef {
            name: ident("my_song"),
            channels: vec![ident("drums")],
            rows: vec![
                SongRow { cells: vec![SongCell::Pattern(ident("pat_a"))] },
                SongRow { cells: vec![SongCell::Pattern(ident("pat_b"))] },
                SongRow { cells: vec![SongCell::Silence] },
            ],
            loop_point: Some(1),
            span: span(),
        }];
        let result = build(&flat, &registry(), &env()).unwrap();
        let td = result.tracker_data.unwrap();
        // Names no longer travel with `TrackerData`. Alphabetical ordering
        // at bank-build time means "my_song" (the only song) is at index 0.
        let song = &td.songs.songs[0];
        assert_eq!(song.channels, 1);
        assert_eq!(song.order.len(), 3);
        assert_eq!(song.order[0][0], Some(0)); // pat_a = index 0
        assert_eq!(song.order[1][0], Some(1)); // pat_b = index 1
        assert_eq!(song.order[2][0], None); // silence
        assert_eq!(song.loop_point, 1);
    }

    #[test]
    fn song_unknown_pattern_is_error() {
        let mut flat = empty_flat();
        flat.patterns = vec![FlatPatternDef {
            name: "exists".to_string(),
            channels: vec![FlatPatternChannel {
                name: "ch".to_string(),
                steps: vec![trigger_step()],
            }],
            span: span(),
        }];
        flat.songs = vec![SongDef {
            name: ident("song"),
            channels: vec![ident("col")],
            rows: vec![SongRow { cells: vec![SongCell::Pattern(ident("no_such_pattern"))] }],
            loop_point: None,
            span: Span { start: 10, end: 20 },
        }];
        let err = build(&flat, &registry(), &env()).unwrap_err();
        assert!(err.message.contains("no_such_pattern"));
    }

    #[test]
    fn song_step_count_mismatch_is_error() {
        let mut flat = empty_flat();
        flat.patterns = vec![
            FlatPatternDef {
                name: "four_steps".to_string(),
                channels: vec![FlatPatternChannel {
                    name: "ch".to_string(),
                    steps: vec![trigger_step(); 4],
                }],
                span: span(),
            },
            FlatPatternDef {
                name: "two_steps".to_string(),
                channels: vec![FlatPatternChannel {
                    name: "ch".to_string(),
                    steps: vec![trigger_step(); 2],
                }],
                span: span(),
            },
        ];
        flat.songs = vec![SongDef {
            name: ident("song"),
            channels: vec![ident("col")],
            rows: vec![
                SongRow { cells: vec![SongCell::Pattern(ident("four_steps"))] },
                SongRow { cells: vec![SongCell::Pattern(ident("two_steps"))] },
            ],
            loop_point: None,
            span: span(),
        }];
        let err = build(&flat, &registry(), &env()).unwrap_err();
        assert!(err.message.contains("steps"));
    }

    #[test]
    fn song_channel_count_mismatch_is_error() {
        let mut flat = empty_flat();
        flat.patterns = vec![
            FlatPatternDef {
                name: "one_ch".to_string(),
                channels: vec![FlatPatternChannel {
                    name: "a".to_string(),
                    steps: vec![trigger_step()],
                }],
                span: span(),
            },
            FlatPatternDef {
                name: "two_ch".to_string(),
                channels: vec![
                    FlatPatternChannel { name: "a".to_string(), steps: vec![trigger_step()] },
                    FlatPatternChannel { name: "b".to_string(), steps: vec![rest_step()] },
                ],
                span: span(),
            },
        ];
        flat.songs = vec![SongDef {
            name: ident("song"),
            channels: vec![ident("col")],
            rows: vec![
                SongRow { cells: vec![SongCell::Pattern(ident("one_ch"))] },
                SongRow { cells: vec![SongCell::Pattern(ident("two_ch"))] },
            ],
            loop_point: None,
            span: span(),
        }];
        let err = build(&flat, &registry(), &env()).unwrap_err();
        assert!(err.message.contains("channels"));
    }

    #[test]
    fn shorter_channels_padded_with_rests() {
        let mut flat = empty_flat();
        flat.patterns = vec![FlatPatternDef {
            name: "uneven".to_string(),
            channels: vec![
                FlatPatternChannel {
                    name: "long".to_string(),
                    steps: vec![trigger_step(); 4],
                },
                FlatPatternChannel {
                    name: "short".to_string(),
                    steps: vec![trigger_step(); 2],
                },
            ],
            span: span(),
        }];
        let result = build(&flat, &registry(), &env()).unwrap();
        let td = result.tracker_data.unwrap();
        let pat = &td.patterns.patterns[0];
        assert_eq!(pat.data[1].len(), 4); // padded to 4
        assert!(!pat.data[1][2].trigger); // pad step is rest
        assert!(!pat.data[1][3].trigger);
    }
}
