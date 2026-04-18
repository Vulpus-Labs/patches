//! Tracker data construction: lift DSL pattern/song blocks into
//! [`patches_core::TrackerData`] and validate MasterSequencer references.

use std::collections::HashMap;

use patches_core::{
    ModuleGraph, Pattern, PatternBank, Song, SongBank, TrackerData, TrackerStep,
};
use patches_dsl::ast::{Scalar, Value};
use patches_dsl::flat::FlatPatch;

use crate::descriptor_bind::ParamConversionError;
use crate::error::{InterpretError, InterpretErrorCode};

/// Build [`TrackerData`] from the pattern and song definitions in a [`FlatPatch`].
///
/// Returns `None` if there are no patterns and no songs.
pub(crate) fn build_tracker_data(
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

/// Convert a DSL [`Value`] to a [`patches_core::ParameterValue`] given the
/// expected [`patches_core::ParameterKind`] from the module descriptor.
pub(crate) fn convert_value(
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
