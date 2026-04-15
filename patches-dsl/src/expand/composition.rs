//! Song/pattern assembly phase of expansion.
//!
//! Converts the song-item / pattern-def AST into flat intermediate forms.
//! The outputs are:
//!
//! - [`AssembledSong`] — a song whose rows have been flattened from
//!   row-groups and play expressions but whose cells still carry
//!   [`SongCell`] values (pattern indices are resolved later once all
//!   patterns are known; see [`index_songs`]).
//! - [`FlatPatternDef`] — a pattern whose step-generators have been
//!   expanded into concrete [`Step`]s.

use std::collections::HashMap;

use patches_core::QName;

use super::{qualify, ExpandError, NameScope};
use crate::ast::{
    Ident, ParamType, PatternDef, PlayAtom, PlayBody, PlayExpr, RowGroup, Scalar, SectionDef,
    SongCell, SongDef, SongItem, SongRow, Span, Step, StepOrGenerator,
};
use crate::flat::{FlatPatternChannel, FlatPatternDef, FlatSongDef, FlatSongRow, PatternIdx};
use crate::provenance::Provenance;
use crate::structural::StructuralCode as Code;

/// A song whose rows still carry [`SongCell`] values; resolved to
/// [`FlatSongDef`] once all patterns are known (see [`index_songs`]).
#[derive(Debug)]
pub(super) struct AssembledSong {
    pub(super) name: QName,
    pub(super) channels: Vec<Ident>,
    pub(super) rows: Vec<SongRow>,
    pub(super) loop_point: Option<usize>,
    pub(super) span: Span,
    /// Call chain at the point this song was assembled (innermost-first).
    pub(super) call_chain: Vec<Span>,
}

/// Final post-pass: convert each [`AssembledSong`] row to a [`FlatSongRow`]
/// with pattern indices into `patterns`. Errors if a cell references an
/// unknown pattern, or if a `ParamRef` survived expansion (which would
/// indicate an expansion bug).
pub(super) fn index_songs(
    patterns: &[FlatPatternDef],
    songs: Vec<AssembledSong>,
) -> Result<Vec<FlatSongDef>, ExpandError> {
    let name_to_idx: HashMap<String, PatternIdx> = patterns
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.to_string(), i))
        .collect();

    let mut out = Vec::with_capacity(songs.len());
    for song in songs {
        let mut rows = Vec::with_capacity(song.rows.len());
        for row in song.rows {
            let mut cells = Vec::with_capacity(row.cells.len());
            for cell in row.cells {
                match cell {
                    SongCell::Silence => cells.push(None),
                    SongCell::Pattern(ident) => match name_to_idx.get(&ident.name) {
                        Some(&idx) => cells.push(Some(idx)),
                        None => {
                            return Err(ExpandError::new(
                                Code::PatternNotFound,
                                ident.span,
                                format!(
                                    "song '{}': pattern '{}' not found",
                                    song.name, ident.name,
                                ),
                            ));
                        }
                    },
                    SongCell::ParamRef { name, span } => {
                        return Err(ExpandError::new(
                            Code::UnresolvedParamRef,
                            span,
                            format!(
                                "song '{}': unresolved `<{}>` param reference after expansion",
                                song.name, name,
                            ),
                        ));
                    }
                }
            }
            rows.push(FlatSongRow {
                cells,
                provenance: Provenance::with_chain(row.span, &song.call_chain),
            });
        }
        let song_provenance = Provenance::with_chain(song.span, &song.call_chain);
        out.push(FlatSongDef {
            name: song.name,
            channels: song.channels,
            rows,
            loop_point: song.loop_point,
            provenance: song_provenance,
        });
    }
    Ok(out)
}

/// Expand a `PatternDef` by resolving all slide generators into concrete steps.
pub(super) fn expand_pattern_def(
    pattern: &PatternDef,
    namespace: Option<&QName>,
    call_chain: &[Span],
) -> FlatPatternDef {
    let channels = pattern
        .channels
        .iter()
        .map(|ch| {
            let steps = expand_steps(&ch.steps);
            FlatPatternChannel {
                name: ch.name.name.clone(),
                steps,
            }
        })
        .collect();
    FlatPatternDef {
        name: qualify(namespace, &pattern.name.name),
        channels,
        provenance: Provenance::with_chain(pattern.span, call_chain),
    }
}

/// Expand a sequence of `StepOrGenerator` into concrete `Step` values.
fn expand_steps(items: &[StepOrGenerator]) -> Vec<Step> {
    let mut out = Vec::new();
    for item in items {
        match item {
            StepOrGenerator::Step(s) => out.push(s.clone()),
            StepOrGenerator::Slide { count, start, end } => {
                let n = *count as usize;
                if n == 0 {
                    continue;
                }
                let step_size = (end - start) / n as f32;
                for i in 0..n {
                    let from = start + step_size * i as f32;
                    let to = start + step_size * (i + 1) as f32;
                    // Only the first subdivision triggers the envelope — the
                    // remainder are ties so the gate stays high through the
                    // slide, matching the ADR's "gate stays high through the
                    // slide" semantics.
                    let trigger = i == 0;
                    out.push(Step {
                        cv1: from,
                        cv2: 0.0,
                        trigger,
                        gate: true,
                        cv1_end: Some(to),
                        cv2_end: None,
                        repeat: 1,
                    });
                }
            }
        }
    }
    out
}

/// Resolve one cell: pattern references are looked up through `scope`; param
/// refs are substituted from `param_env` (and checked against `param_types`).
fn subst_song_cell(
    cell: &SongCell,
    param_env: &HashMap<String, Scalar>,
    param_types: &HashMap<String, ParamType>,
    scope: &NameScope<'_>,
) -> Result<SongCell, ExpandError> {
    match cell {
        SongCell::Silence => Ok(SongCell::Silence),
        SongCell::Pattern(ident) => {
            let resolved = scope
                .resolve_pattern(&ident.name)
                .map(|q| q.to_string())
                .unwrap_or_else(|| ident.name.clone());
            Ok(SongCell::Pattern(Ident {
                name: resolved,
                span: ident.span,
            }))
        }
        SongCell::ParamRef { name, span } => {
            if let Some(ty) = param_types.get(name.as_str()) {
                if *ty != ParamType::Pattern {
                    return Err(ExpandError::new(
                        Code::ParamTypeMismatch,
                        *span,
                        format!(
                            "song cell '<{}>': param is {}-typed, expected pattern",
                            name,
                            super::param_type_name(ty),
                        ),
                    ));
                }
            }
            match param_env.get(name.as_str()) {
                Some(Scalar::Str(s)) => {
                    let resolved = scope
                        .resolve_pattern(s)
                        .map(|q| q.to_string())
                        .unwrap_or_else(|| s.clone());
                    Ok(SongCell::Pattern(Ident {
                        name: resolved,
                        span: *span,
                    }))
                }
                Some(other) => Err(ExpandError::new(
                    Code::ParamTypeMismatch,
                    *span,
                    format!(
                        "song cell param '<{}>': expected a pattern name, got {:?}",
                        name, other,
                    ),
                )),
                None => Err(ExpandError::new(
                    Code::UnresolvedParamRef,
                    *span,
                    format!("unresolved param '<{}>' in song cell", name),
                )),
            }
        }
    }
}

/// Flatten one [`RowGroup`] tree into a sequence of concrete [`SongRow`]s,
/// validating lane count and resolving cells.
fn flatten_row_groups(
    groups: &[RowGroup],
    lanes: &[Ident],
    param_env: &HashMap<String, Scalar>,
    param_types: &HashMap<String, ParamType>,
    scope: &NameScope<'_>,
    out: &mut Vec<SongRow>,
) -> Result<(), ExpandError> {
    for g in groups {
        match g {
            RowGroup::Row(row) => {
                if row.cells.len() != lanes.len() {
                    return Err(ExpandError::new(
                        Code::RowLaneMismatch,
                        row.span,
                        format!(
                            "row has {} cells but song declares {} lane(s)",
                            row.cells.len(),
                            lanes.len(),
                        ),
                    ));
                }
                let cells = row
                    .cells
                    .iter()
                    .map(|c| subst_song_cell(c, param_env, param_types, scope))
                    .collect::<Result<Vec<_>, _>>()?;
                out.push(SongRow {
                    cells,
                    span: row.span,
                });
            }
            RowGroup::Repeat { body, count, .. } => {
                let start = out.len();
                flatten_row_groups(body, lanes, param_env, param_types, scope, out)?;
                let inner_len = out.len() - start;
                for _ in 1..*count {
                    for i in 0..inner_len {
                        out.push(out[start + i].clone());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Evaluate a [`PlayExpr`] against the per-song section table, appending the
/// produced rows to `out`.
fn eval_play_expr(
    expr: &PlayExpr,
    section_rows: &HashMap<String, Vec<SongRow>>,
    out: &mut Vec<SongRow>,
) -> Result<(), ExpandError> {
    for term in &expr.terms {
        let mut buf: Vec<SongRow> = Vec::new();
        match &term.atom {
            PlayAtom::Ref(ident) => match section_rows.get(&ident.name) {
                Some(rows) => buf.extend(rows.iter().cloned()),
                None => {
                    return Err(ExpandError::new(
                        Code::UnknownSection,
                        ident.span,
                        format!("unknown section '{}' in play expression", ident.name),
                    ));
                }
            },
            PlayAtom::Group(inner) => eval_play_expr(inner, section_rows, &mut buf)?,
        }
        for _ in 0..term.repeat {
            out.extend(buf.iter().cloned());
        }
    }
    Ok(())
}

/// Flatten a [`SongDef`] into an [`AssembledSong`] plus any song-local inline
/// [`FlatPatternDef`]s. Also enforces scope/duplication rules for song items.
pub(super) fn flatten_song(
    song: &SongDef,
    namespace: Option<&QName>,
    param_env: &HashMap<String, Scalar>,
    param_types: &HashMap<String, ParamType>,
    parent_scope: &NameScope<'_>,
    call_chain: &[Span],
) -> Result<(AssembledSong, Vec<FlatPatternDef>), ExpandError> {
    let song_ns = qualify(namespace, &song.name.name);

    // Pass 1: collect song-local sections and inline patterns, flagging dups.
    let mut local_sections: HashMap<String, &SectionDef> = HashMap::new();
    let mut local_patterns: Vec<&PatternDef> = Vec::new();
    for item in &song.items {
        match item {
            SongItem::Section(sd) => {
                if local_sections.insert(sd.name.name.clone(), sd).is_some() {
                    return Err(ExpandError::new(
                        Code::DuplicateSection,
                        sd.span,
                        format!("duplicate section '{}' in song", sd.name.name),
                    ));
                }
            }
            SongItem::Pattern(pd) => {
                if local_patterns.iter().any(|p| p.name.name == pd.name.name) {
                    return Err(ExpandError::new(
                        Code::DuplicateInlinePattern,
                        pd.span,
                        format!("duplicate inline pattern '{}' in song", pd.name.name),
                    ));
                }
                local_patterns.push(pd);
            }
            _ => {}
        }
    }

    // Build the song-local scope: its patterns qualify under `song_ns`.
    let scope = NameScope::song_scope(parent_scope, &local_patterns, &song_ns);

    // Flatten each song-local section into its row list (and cache on demand).
    let mut section_rows: HashMap<String, Vec<SongRow>> = HashMap::new();
    for (name, sd) in &local_sections {
        let mut rows = Vec::new();
        flatten_row_groups(&sd.body, &song.lanes, param_env, param_types, &scope, &mut rows)?;
        section_rows.insert(name.clone(), rows);
    }

    // Eagerly flatten file-level sections available via the parent scope.
    // (They expand against this song's lanes, so the same section may be
    // reused across songs with different lane counts.)
    let top_sections = parent_scope.top_level_sections();
    for (name, sd) in top_sections {
        if local_sections.contains_key(&name) {
            continue;
        }
        let mut rows = Vec::new();
        flatten_row_groups(&sd.body, &song.lanes, param_env, param_types, &scope, &mut rows)?;
        section_rows.insert(name, rows);
    }

    // Pass 2: process play / loop items in source order.
    let mut rows: Vec<SongRow> = Vec::new();
    let mut loop_point: Option<usize> = None;

    for item in &song.items {
        match item {
            SongItem::Section(_) | SongItem::Pattern(_) => {}
            SongItem::LoopMarker(span) => {
                if loop_point.is_some() {
                    return Err(ExpandError::new(
                        Code::MultipleLoopMarkers,
                        *span,
                        "multiple @loop markers in song".to_owned(),
                    ));
                }
                loop_point = Some(rows.len());
            }
            SongItem::Play(body) => match body {
                PlayBody::Inline { body, .. } => {
                    flatten_row_groups(body, &song.lanes, param_env, param_types, &scope, &mut rows)?;
                }
                PlayBody::NamedInline { name, body, span } => {
                    if section_rows.contains_key(&name.name) {
                        return Err(ExpandError::new(
                            Code::SectionAlreadyDefined,
                            *span,
                            format!("section '{}' already defined in song", name.name),
                        ));
                    }
                    let mut section_only = Vec::new();
                    flatten_row_groups(
                        body,
                        &song.lanes,
                        param_env,
                        param_types,
                        &scope,
                        &mut section_only,
                    )?;
                    rows.extend(section_only.iter().cloned());
                    section_rows.insert(name.name.clone(), section_only);
                }
                PlayBody::Expr(expr) => {
                    eval_play_expr(expr, &section_rows, &mut rows)?;
                }
            },
        }
    }

    // Emit flat pattern defs for song-local inline patterns.
    let flat_patterns: Vec<FlatPatternDef> = local_patterns
        .iter()
        .map(|p| expand_pattern_def(p, Some(&song_ns), call_chain))
        .collect();

    Ok((
        AssembledSong {
            name: song_ns,
            channels: song.lanes.clone(),
            rows,
            loop_point,
            span: song.span,
            call_chain: call_chain.to_vec(),
        },
        flat_patterns,
    ))
}
