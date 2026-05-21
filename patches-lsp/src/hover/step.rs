//! Hover content for pattern-row step cells under the ADR 0077 unified
//! step-event grammar: tie (`_`), `*N` repeat marker, `/value` step-to,
//! `value>` slide-open, `>_` tie-flow, `>value` slide-close-in-tick,
//! and bare `value` cells that close a preceding open slide.
//!
//! All handlers run the row-build pass [`resolve_step_effects`] over
//! the channel's step run so the hover text reflects the same
//! semantics the audio thread sees — the row-build pass is the single
//! authoritative classification (ADR 0077 § "Row-build pass").

use patches_core::{resolve_step_effects, RollSpec, SlideOpen, StepEffect, StepKind, TrackerStep};
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

use crate::lsp_util::byte_offset_to_position;

fn node_range(node: Node<'_>, source: &str, line_starts: &[usize]) -> Range {
    let start = byte_offset_to_position(source, line_starts, node.start_byte());
    let end = byte_offset_to_position(source, line_starts, node.end_byte());
    Range::new(start, end)
}

/// Walk a `channel_row` and return its step nodes in source order,
/// flattening across any `channel_row_cont` (`|`) continuations.
///
/// Post-ticket-0948 the grammar exposes `step` nodes directly under
/// `channel_row` (the `step_or_generator` wrapper went away when the
/// `slide(n, …)` macro was abolished).
fn collect_steps<'tree>(channel_row: Node<'tree>) -> Vec<Node<'tree>> {
    let mut out = Vec::new();
    let mut cursor = channel_row.walk();
    for child in channel_row.children(&mut cursor) {
        match child.kind() {
            "step" => out.push(child),
            "channel_row_cont" => {
                let mut inner = child.walk();
                for s in child.children(&mut inner) {
                    if s.kind() == "step" {
                        out.push(s);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Decode the source text of a `step` node into a runtime [`TrackerStep`].
/// Only fields the row-build pass needs to classify a cell are populated;
/// cv1/cv2 are filled where they are read off the source text without
/// units (slide-target values and the like still default to 0.0).
fn decode_step(step: Node<'_>, source: &str) -> TrackerStep {
    let Some(inner) = step.named_child(0) else {
        return TrackerStep::default();
    };
    match inner.kind() {
        "step_rest" => TrackerStep::default(),
        "step_tie" => TrackerStep {
            gate: true,
            kind: StepKind::Tie,
            ..TrackerStep::default()
        },
        "step_tie_flow" => TrackerStep {
            gate: true,
            kind: StepKind::TieFlow,
            ..TrackerStep::default()
        },
        "step_step_to" => {
            // `/value` with optional `>cv1_end` slide target.
            let has_slide = inner
                .named_children(&mut inner.walk())
                .any(|c| c.kind() == "step_slide_target");
            let kind = if has_slide {
                StepKind::StepToSlide { cv1_end: 0.0, cv2: None }
            } else {
                StepKind::StepTo { cv2: None }
            };
            TrackerStep {
                gate: true,
                kind,
                ..TrackerStep::default()
            }
        }
        "step_slide_close" => TrackerStep {
            gate: true,
            kind: StepKind::SlideCloseInTick { cv2: None },
            ..TrackerStep::default()
        },
        "step_slide_open" => TrackerStep {
            trigger: true,
            gate: true,
            kind: StepKind::SlideOpen,
            ..TrackerStep::default()
        },
        "step_valued" => decode_step_valued(inner, source),
        _ => TrackerStep::default(),
    }
}

/// Decode a `step_valued` parent node (whose sole named child is one
/// of `step_valued_slide` / `step_valued_note`, per ticket 0950).
fn decode_step_valued(parent: Node<'_>, source: &str) -> TrackerStep {
    let Some(inner) = parent.named_child(0) else {
        return TrackerStep::default();
    };
    let kind = match inner.kind() {
        "step_valued_slide" => StepKind::SlideSugar { cv1_end: 0.0, cv2_end: None },
        "step_valued_note" => {
            let mut repeat: u8 = 1;
            let mut cursor = inner.walk();
            for child in inner.children(&mut cursor) {
                if child.kind() == "step_repeat" {
                    if let Some(nat) = child.named_child(0) {
                        let text = &source[nat.start_byte()..nat.end_byte()];
                        if let Ok(n) = text.parse::<u8>() {
                            repeat = n;
                        }
                    }
                }
            }
            StepKind::Note { repeat }
        }
        _ => return TrackerStep::default(),
    };
    TrackerStep {
        trigger: true,
        gate: true,
        kind,
        ..TrackerStep::default()
    }
}

/// Build the resolved step run for a channel row and find the index of
/// the step containing `target` (matched by source byte range).
fn resolved_row(
    channel_row: Node<'_>,
    source: &str,
    target: Node<'_>,
) -> (Vec<TrackerStep>, Option<usize>) {
    let step_nodes = collect_steps(channel_row);
    let mut steps: Vec<TrackerStep> = step_nodes
        .iter()
        .map(|n| decode_step(*n, source))
        .collect();
    let _ = resolve_step_effects(&mut steps);
    let target_start = target.start_byte();
    let target_end = target.end_byte();
    let idx = step_nodes.iter().position(|n| {
        n.start_byte() <= target_start && target_end <= n.end_byte()
    });
    (steps, idx)
}

/// Hover for a tie cell (`_`). Distinguishes the three meanings via the
/// row-build resolution: `AbsorbedRoll` ⇒ roll continuation,
/// `SlideFlow` ⇒ slide continuation, otherwise sustain.
pub(crate) fn hover_for_tie(
    tie_node: Node<'_>,
    channel_row: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let (steps, idx) = resolved_row(channel_row, source, tie_node);
    let i = idx?;
    let step = steps.get(i)?;
    let value = match &step.effect {
        StepEffect::AbsorbedRoll => {
            // Find the anchor backwards.
            let anchor = find_roll_anchor(&steps, i)
                .and_then(|j| match &steps[j].effect {
                    StepEffect::StartNote { roll: Some(r), .. } => Some((j, r.clone())),
                    _ => None,
                });
            match anchor {
                Some((_, r)) => format!(
                    "**`_` (roll continuation)**\n\n\
                     Extends the preceding `*{n}` roll across this tick \
                     (E152 tie-spread).\n\n\
                     Anchor `*{n}` spans {span} ticks; the {n} sub-triggers \
                     are spaced `tick * {span} / {n}` samples apart.\n\n\
                     The tie cell itself emits no gate change or trigger — the \
                     anchor's in-flight schedule owns the channel.",
                    n = r.count,
                    span = r.span,
                ),
                None => "**`_` (roll continuation)**".to_owned(),
            }
        }
        StepEffect::SlideFlow => "**`_` (slide-flow continuation)**\n\n\
             Absorbed into the preceding slide's ramp schedule — no \
             trigger, no gate change. The slide's cv ramp continues \
             through this tick toward the close cell's endpoint."
            .to_owned(),
        _ => "**`_` (sustain tie)**\n\n\
             Hold the gate high; emit no new trigger. cv1/cv2 carry over \
             from the previous step.\n\n\
             (When the previous step opened a roll (`*N`) or a slide \
             (`value>` / `>_`) this tie is absorbed into that modifier.)"
            .to_owned(),
    };
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(node_range(tie_node, source, line_starts)),
    })
}

/// Hover for a `*N` repeat marker. Reports the repeat count and the
/// roll's resolved span (from row-build absorption of trailing ties).
pub(crate) fn hover_for_repeat(
    repeat_node: Node<'_>,
    channel_row: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let (steps, idx) = resolved_row(channel_row, source, repeat_node);
    let i = idx?;
    let step = steps.get(i)?;
    let r = match &step.effect {
        StepEffect::StartNote { roll: Some(r), .. } => r.clone(),
        _ => RollSpec { count: 1, span: 1 },
    };
    let value = if r.span > 1 {
        let absorbed = r.span - 1;
        let cell = if absorbed == 1 { "tie" } else { "ties" };
        format!(
            "**`*{n}` (rolled across {span} ticks)**\n\n\
             Fires {n} evenly-spaced sub-triggers over the anchor \
             tick plus {absorbed} absorbed-by-roll {cell} \
             (E152 tie-spread).\n\n\
             Interval = `tick * {span} / {n}` samples; \
             gate articulates at 0.8 of each interval.",
            n = r.count,
            span = r.span,
        )
    } else {
        format!(
            "**`*{n}` (single-tick roll)**\n\n\
             Subdivides the current tick into {n} evenly-spaced \
             sub-triggers (interval = `tick / {n}`). Follow with \
             `_` cells to spread the roll across multiple ticks \
             (E152).",
            n = r.count,
        )
    };
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(node_range(repeat_node, source, line_starts)),
    })
}

fn find_roll_anchor(steps: &[TrackerStep], from: usize) -> Option<usize> {
    for j in (0..from).rev() {
        if let StepEffect::StartNote { roll: Some(_), .. } = &steps[j].effect {
            return Some(j);
        }
    }
    None
}

/// Source text of the value portion of an ADR 0077 slide cell, stripped
/// of the cell's punctuation. Used for "step cv to G4" / "ramp to G4"
/// hover phrasing — we don't re-parse the numeric cv, just echo whatever
/// the user wrote (note, float, unit literal).
///
/// Falls back to the full cell text if the inner value child isn't
/// recoverable; the hover still reads correctly in that case.
fn cell_value_text<'a>(cell: Node<'_>, source: &'a str) -> &'a str {
    // Post-0950 the numeric primaries are `float_unit` / `float_lit` /
    // `int_lit` (the `step_*` numeric duplicates were collapsed).
    let value_kinds = ["step_note", "float_unit", "float_lit", "int_lit"];
    let mut cursor = cell.walk();
    for child in cell.named_children(&mut cursor) {
        if value_kinds.contains(&child.kind()) {
            return &source[child.start_byte()..child.end_byte()];
        }
    }
    &source[cell.start_byte()..cell.end_byte()]
}

/// True when the channel had an open slide leading into the cell at
/// index `i`. Walks back through `SlideFlow` / `OpenSlide` cells and
/// stops on a slide-opening `StartNote { slide: Some(_), .. }` —
/// matching the row-build pass's channel-state walk.
fn slide_open_leading_in(steps: &[TrackerStep], i: usize) -> Option<&SlideOpen> {
    for j in (0..i).rev() {
        match &steps[j].effect {
            StepEffect::SlideFlow | StepEffect::OpenSlide { .. } => continue,
            StepEffect::StartNote { slide: Some(s), .. } => return Some(s),
            _ => return None,
        }
    }
    None
}

/// Hover for a `/value` step-to cell. Renders the resolved `StepCv` /
/// `StepCvSlide` effect; mentions when the cell also closes a
/// preceding open slide. When the cell carries a `>cv1_end` target,
/// describes the snap + in-tick ramp.
pub(crate) fn hover_for_step_to(
    cell: Node<'_>,
    channel_row: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let (steps, idx) = resolved_row(channel_row, source, cell);
    let i = idx?;
    let value = cell_value_text(cell, source);
    let close_note = if slide_open_leading_in(&steps, i).is_some() {
        "\n\nThe preceding open slide closes at the tick boundary at \
         this cell's value."
    } else {
        ""
    };
    let has_slide = matches!(steps[i].effect, StepEffect::StepCvSlide { .. });
    let text = if has_slide {
        let raw = &source[cell.start_byte()..cell.end_byte()];
        format!(
            "**`{raw}` (step cv to {value}, then ramp within this tick, no retrigger)**\n\n\
             Snap cv1 to `{value}` at tick start (no fresh trigger), then ramp \
             cv1 from `{value}` to the `>cv1_end` target across this tick. \
             Mirrors the `value>value` sugar form, but without retriggering.{close_note}",
        )
    } else {
        format!(
            "**`/{value}` (step cv to {value}, no retrigger)**\n\n\
             Snap cv1 to `{value}` at the tick boundary; gate stays high; \
             no fresh trigger.{close_note}",
        )
    };
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: text,
        }),
        range: Some(node_range(cell, source, line_starts)),
    })
}

/// Hover for a `value>` slide-open cell. Reports trigger + open slide
/// and notes that the close cell may sit further down the row.
pub(crate) fn hover_for_slide_open(
    cell: Node<'_>,
    channel_row: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let (steps, idx) = resolved_row(channel_row, source, cell);
    let i = idx?;
    let value = cell_value_text(cell, source);
    let close_phrase = match &steps[i].effect {
        StepEffect::StartNote { slide: Some(s), .. } => {
            if s.closes_at_boundary {
                format!(
                    "Closes at the next non-`_` cell (`value`, `/value`, \
                     `value>`), at the tick boundary entering that cell. \
                     Span resolved to {} tick(s).",
                    s.span
                )
            } else {
                format!(
                    "Closes inside the next `>value` cell (close-in-tick). \
                     Span resolved to {} tick(s).",
                    s.span
                )
            }
        }
        _ => "Closes at next non-`_` cell.".to_owned(),
    };
    let text = format!(
        "**`{value}>` (trigger + open slide; closes at next non-`_` cell)**\n\n\
         Snap cv1 to `{value}` at the start of this tick and fire a fresh \
         trigger. The slide ramp continues across `_` and `>_` flow cells \
         until the row-build close rule fires.\n\n{close_phrase}",
    );
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: text,
        }),
        range: Some(node_range(cell, source, line_starts)),
    })
}

/// Hover for a `>_` tie-flow cell. Always describes "slide flow" — the
/// cell either opens a slide from the channel's current cv or extends
/// an already-open slide.
pub(crate) fn hover_for_tie_flow(
    cell: Node<'_>,
    channel_row: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let (steps, idx) = resolved_row(channel_row, source, cell);
    let i = idx?;
    let body = match &steps[i].effect {
        StepEffect::OpenSlide { .. } => {
            "**`>_` (slide flow — opens slide from current cv)**\n\n\
             No trigger; no cv snap. Opens a new slide from the channel's \
             current cv, ramping toward the close cell's endpoint."
        }
        StepEffect::SlideFlow => {
            "**`>_` (slide flow — extends open slide)**\n\n\
             No trigger; no cv snap. Extends the preceding slide's ramp \
             across this tick."
        }
        _ => "**`>_` (slide flow)**",
    };
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: body.to_owned(),
        }),
        range: Some(node_range(cell, source, line_starts)),
    })
}

/// Hover for a `>value` slide-close-in-tick cell.
pub(crate) fn hover_for_slide_close(
    cell: Node<'_>,
    channel_row: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let (steps, idx) = resolved_row(channel_row, source, cell);
    let i = idx?;
    let value = cell_value_text(cell, source);
    let span_note = match &steps[i].effect {
        StepEffect::SlideCloseInTick { .. } => {
            slide_open_leading_in(&steps, i)
                .map(|s| format!(
                    "\n\nSlide span resolved to {} tick(s); ramp finishes \
                     at `{value}` at the end of this tick.",
                    s.span
                ))
                .unwrap_or_default()
        }
        _ => String::new(),
    };
    let text = format!(
        "**`>{value}` (ramp to {value} within this tick, no retrigger)**\n\n\
         Cv1 ramps from the channel's current value to `{value}` across this \
         tick; no fresh trigger.{span_note}",
    );
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: text,
        }),
        range: Some(node_range(cell, source, line_starts)),
    })
}

/// Hover for a bare `value` (`step_valued_note`) cell. Only renders
/// when the cell closes a preceding open slide — that's the case
/// where the row's lead-in shape is non-obvious from the cell alone
/// (ADR 0077: "bare `value` is always locally readable as a fresh
/// trigger").
pub(crate) fn hover_for_note(
    cell: Node<'_>,
    channel_row: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let (steps, idx) = resolved_row(channel_row, source, cell);
    let i = idx?;
    slide_open_leading_in(&steps, i)?;
    let value = cell_value_text(cell, source);
    let text = format!(
        "**`{value}` (fresh trigger; preceding slide closes at boundary at {value})**\n\n\
         The preceding open slide ramps to `{value}` and lands at the tick \
         boundary entering this cell; this cell then fires a fresh trigger \
         at `{value}` on its own tick.",
    );
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: text,
        }),
        range: Some(node_range(cell, source, line_starts)),
    })
}
