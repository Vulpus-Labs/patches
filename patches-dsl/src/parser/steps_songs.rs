//! Pest-tree walkers for step/pattern/song/row/play nodes.
//!
//! Covers `step`, `channel_row`, `pattern_block`,
//! `song_row`/`row_group`/`repeat_group`/`row_seq`, and the
//! play-expression hierarchy plus `song_block`.

use pest::iterators::Pair;

use crate::ast::{
    Ident, PatternChannel, PatternDef, PlayAtom, PlayBody, PlayExpr, PlayTerm, RowGroup, SongCell,
    SongDef, SongItem, SongRow, Step, StepKind,
};

use super::decls::build_section_def;
use super::error::ParseError;
use super::expressions::build_ident;
use super::literals::{parse_step_float, parse_step_note, parse_step_unit};
use super::{span_of, Rule};

/// Parse a cv1 value from a step primary pair (step_note, step_trigger,
/// float_lit, int_lit, float_unit). Ticket 0950 collapsed the
/// step-specific numeric duplicates onto the canonical `*_lit` rules.
fn parse_cv1_value(pair: &Pair<'_, Rule>) -> Result<f32, ParseError> {
    let span = span_of(pair);
    match pair.as_rule() {
        Rule::step_note => parse_step_note(pair.as_str(), span),
        Rule::step_trigger => Ok(0.0),
        Rule::float_lit | Rule::int_lit => parse_step_float(pair.as_str(), span),
        Rule::float_unit => parse_step_unit(pair.as_str(), span),
        _ => unreachable!("unexpected cv1 rule: {:?}", pair.as_rule()),
    }
}

/// Parse a slide target value from a step_slide_target's inner pair.
fn parse_slide_target_value(pair: Pair<'_, Rule>) -> Result<f32, ParseError> {
    let inner = pair.into_inner().next().unwrap();
    let span = span_of(&inner);
    match inner.as_rule() {
        Rule::step_note => parse_step_note(inner.as_str(), span),
        Rule::float_lit | Rule::int_lit => parse_step_float(inner.as_str(), span),
        Rule::float_unit => parse_step_unit(inner.as_str(), span),
        _ => unreachable!("unexpected slide target rule: {:?}", inner.as_rule()),
    }
}

/// Parse the `:value` cv2 tail on a slide cell (`step_cv2_tail`).
/// Inner pair is `float_unit | float_lit | int_lit`.
fn parse_cv2_tail(pair: Pair<'_, Rule>) -> Result<f32, ParseError> {
    let inner = pair.into_inner().next().unwrap();
    let span = span_of(&inner);
    match inner.as_rule() {
        Rule::float_lit | Rule::int_lit => parse_step_float(inner.as_str(), span),
        Rule::float_unit => parse_step_unit(inner.as_str(), span),
        _ => unreachable!("unexpected cv2 tail rule: {:?}", inner.as_rule()),
    }
}

/// Parse a cv2 value (no slide target) from a step_cv2's first inner
/// primary pair (`float_unit | float_lit | int_lit`).
fn parse_cv2_primary(pair: &Pair<'_, Rule>) -> Result<f32, ParseError> {
    let span = span_of(pair);
    match pair.as_rule() {
        Rule::float_lit | Rule::int_lit => parse_step_float(pair.as_str(), span),
        Rule::float_unit => parse_step_unit(pair.as_str(), span),
        _ => unreachable!("unexpected cv2 rule: {:?}", pair.as_rule()),
    }
}

/// Walk a `step_cv2` pair, returning `(cv2, cv2_end)`. cv2 is the first
/// primary value (`float_unit | float_lit | int_lit`); `cv2_end` is the
/// optional `>endpoint` slide target. Shared between the two
/// `step_valued_*` arms.
fn parse_step_cv2(child: Pair<'_, Rule>) -> Result<(f32, Option<f32>), ParseError> {
    let mut cv2_it = child.into_inner();
    let cv2_val_pair = cv2_it.next().unwrap();
    let cv2 = parse_cv2_primary(&cv2_val_pair)?;
    let cv2_end = match cv2_it.next() {
        Some(slide_pair) => Some(parse_slide_target_value(slide_pair)?),
        None => None,
    };
    Ok((cv2, cv2_end))
}

fn build_step(pair: Pair<'_, Rule>) -> Result<Step, ParseError> {
    // pair.as_rule() == Rule::step
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::step_rest => Ok(Step::default()),
        Rule::step_tie => Ok(Step {
            gate: true,
            kind: StepKind::Tie,
            ..Step::default()
        }),
        Rule::step_tie_flow => Ok(Step {
            gate: true,
            kind: StepKind::TieFlow,
            ..Step::default()
        }),
        Rule::step_step_to => {
            // step_step_to = ${ "/" ~ primary ~ step_slide_target? ~ step_cv2_tail? }
            let mut it = inner.into_inner();
            let val_pair = it.next().unwrap();
            let cv1 = parse_cv1_value(&val_pair)?;
            let mut cv1_end: Option<f32> = None;
            let mut cv2: Option<f32> = None;
            for child in it {
                match child.as_rule() {
                    Rule::step_slide_target => {
                        cv1_end = Some(parse_slide_target_value(child)?);
                    }
                    Rule::step_cv2_tail => {
                        cv2 = Some(parse_cv2_tail(child)?);
                    }
                    other => unreachable!("unexpected rule in step_step_to: {other:?}"),
                }
            }
            let kind = match cv1_end {
                Some(end) => StepKind::StepToSlide { cv1_end: end, cv2 },
                None => StepKind::StepTo { cv2 },
            };
            Ok(Step {
                cv1,
                cv2: cv2.unwrap_or(0.0),
                gate: true,
                kind,
                ..Step::default()
            })
        }
        Rule::step_slide_close => {
            // step_slide_close = ${ ">" ~ primary ~ step_cv2_tail? }
            let mut it = inner.into_inner();
            let val_pair = it.next().unwrap();
            let cv1 = parse_cv1_value(&val_pair)?;
            let cv2 = match it.next() {
                Some(tail) => Some(parse_cv2_tail(tail)?),
                None => None,
            };
            Ok(Step {
                cv1,
                cv2: cv2.unwrap_or(0.0),
                gate: true,
                kind: StepKind::SlideCloseInTick { cv2 },
                ..Step::default()
            })
        }
        Rule::step_slide_open => {
            // step_slide_open = ${ primary ~ step_cv2_tail? ~ ">" ~ !primary }
            let mut it = inner.into_inner();
            let val_pair = it.next().unwrap();
            let cv1 = parse_cv1_value(&val_pair)?;
            let cv2 = match it.next() {
                Some(tail) => parse_cv2_tail(tail)?,
                None => 0.0,
            };
            Ok(Step {
                cv1,
                cv2,
                trigger: true,
                gate: true,
                kind: StepKind::SlideOpen,
            })
        }
        Rule::step_valued => {
            // step_valued = { step_valued_slide | step_valued_note }
            // The two alternatives are mutually exclusive at the
            // grammar level (ticket 0950), so no post-hoc
            // classification or defensive `cv1_end + repeat` check is
            // needed here — the inner rule names the shape.
            let inner = inner.into_inner().next().unwrap();
            match inner.as_rule() {
                Rule::step_valued_slide => build_step_valued_slide(inner),
                Rule::step_valued_note => build_step_valued_note(inner),
                other => unreachable!("unexpected rule in step_valued: {other:?}"),
            }
        }
        _ => unreachable!("unexpected rule in step: {:?}", inner.as_rule()),
    }
}

/// Build the `value>cv1_end[:cv2]` slide-sugar shape.
fn build_step_valued_slide(pair: Pair<'_, Rule>) -> Result<Step, ParseError> {
    // step_valued_slide = ${ primary ~ step_slide_target ~ step_cv2? }
    let mut it = pair.into_inner();
    let cv1_pair = it.next().unwrap();
    let cv1 = parse_cv1_value(&cv1_pair)?;
    let slide_pair = it.next().unwrap();
    debug_assert_eq!(slide_pair.as_rule(), Rule::step_slide_target);
    let cv1_end = parse_slide_target_value(slide_pair)?;
    let (cv2, cv2_end) = match it.next() {
        Some(cv2_pair) => {
            debug_assert_eq!(cv2_pair.as_rule(), Rule::step_cv2);
            parse_step_cv2(cv2_pair)?
        }
        None => (0.0, None),
    };
    Ok(Step {
        cv1,
        cv2,
        trigger: true,
        gate: true,
        kind: StepKind::SlideSugar { cv1_end, cv2_end },
    })
}

/// Build the `value[:cv2][*N]` note shape.
fn build_step_valued_note(pair: Pair<'_, Rule>) -> Result<Step, ParseError> {
    // step_valued_note = ${ primary ~ step_cv2? ~ step_repeat? }
    let mut it = pair.into_inner();
    let cv1_pair = it.next().unwrap();
    let cv1 = parse_cv1_value(&cv1_pair)?;
    let mut cv2: f32 = 0.0;
    let mut repeat: u8 = 1;
    for child in it {
        match child.as_rule() {
            Rule::step_cv2 => {
                let (v, end) = parse_step_cv2(child)?;
                cv2 = v;
                // step_valued_note disallows a cv1 slide target, but
                // cv2 may still ramp (`:cv2>endpoint`); the cv2_end is
                // currently unused for the note shape because runtime
                // semantics treat note cv2 as a static value — fall
                // back to ignoring the ramp here to preserve existing
                // behaviour.
                let _ = end;
            }
            Rule::step_repeat => {
                let nat_pair = child.into_inner().next().unwrap();
                let span = span_of(&nat_pair);
                repeat = nat_pair.as_str().parse::<u8>().map_err(|_| ParseError {
                    span,
                    message: format!("invalid repeat count: {:?}", nat_pair.as_str()),
                })?;
            }
            other => unreachable!("unexpected rule in step_valued_note: {other:?}"),
        }
    }
    Ok(Step {
        cv1,
        cv2,
        trigger: true,
        gate: true,
        kind: StepKind::Note { repeat },
    })
}

fn build_channel_row(pair: Pair<'_, Rule>) -> Result<PatternChannel, ParseError> {
    // channel_row = { ident ~ ":" ~ step* ~ channel_row_cont* }
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let mut steps: Vec<Step> = Vec::new();

    for child in it {
        match child.as_rule() {
            Rule::step => steps.push(build_step(child)?),
            Rule::channel_row_cont => {
                // channel_row_cont = { "|" ~ step* }
                for s in child.into_inner() {
                    steps.push(build_step(s)?);
                }
            }
            _ => unreachable!("unexpected rule in channel_row: {:?}", child.as_rule()),
        }
    }

    Ok(PatternChannel { name, steps })
}

pub(super) fn build_pattern_block(pair: Pair<'_, Rule>) -> Result<PatternDef, ParseError> {
    // pattern_block = { "pattern" ~ ident ~ "{" ~ channel_row+ ~ "}" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let channels: Vec<PatternChannel> =
        it.map(build_channel_row).collect::<Result<_, _>>()?;
    Ok(PatternDef { name, channels, span })
}

fn build_row_cell(pair: Pair<'_, Rule>) -> SongCell {
    // row_cell = ${ song_silence | param_ref | ident }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::song_silence => SongCell::Silence,
        Rule::ident => SongCell::Pattern(build_ident(inner)),
        Rule::param_ref => {
            let span = span_of(&inner);
            let name = inner.into_inner().next().unwrap().as_str().to_owned();
            SongCell::ParamRef { name, span }
        }
        _ => unreachable!("unexpected rule in row_cell: {:?}", inner.as_rule()),
    }
}

fn build_song_row(pair: Pair<'_, Rule>) -> SongRow {
    // song_row = ${ row_cell ~ ("," ~ row_cell)* }
    let span = span_of(&pair);
    let cells: Vec<SongCell> = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::row_cell)
        .map(build_row_cell)
        .collect();
    SongRow { cells, span }
}

fn build_row_group(pair: Pair<'_, Rule>) -> Result<RowGroup, ParseError> {
    // row_group = ${ repeat_group | song_row }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::song_row => Ok(RowGroup::Row(build_song_row(inner))),
        Rule::repeat_group => build_repeat_group(inner),
        _ => unreachable!("unexpected rule in row_group: {:?}", inner.as_rule()),
    }
}

fn build_repeat_group(pair: Pair<'_, Rule>) -> Result<RowGroup, ParseError> {
    // repeat_group = ${ "(" ~ row_seq ~ ")" ~ "*" ~ nat }
    let span = span_of(&pair);
    let mut body: Option<Vec<RowGroup>> = None;
    let mut count: Option<u32> = None;
    for child in pair.into_inner() {
        match child.as_rule() {
            Rule::row_seq => body = Some(build_row_seq(child)?),
            Rule::nat => {
                let n_span = span_of(&child);
                let n: u32 = child.as_str().parse().map_err(|_| ParseError {
                    span: n_span,
                    message: format!("invalid repeat count: {:?}", child.as_str()),
                })?;
                if n == 0 {
                    return Err(ParseError {
                        span: n_span,
                        message: "row-group repeat count must be positive".to_owned(),
                    });
                }
                count = Some(n);
            }
            _ => unreachable!("unexpected rule in repeat_group: {:?}", child.as_rule()),
        }
    }
    Ok(RowGroup::Repeat {
        body: body.unwrap(),
        count: count.unwrap(),
        span,
    })
}

pub(super) fn build_row_seq(pair: Pair<'_, Rule>) -> Result<Vec<RowGroup>, ParseError> {
    // row_seq = ${ ... row_group (row_sep row_group)* ... }
    pair.into_inner()
        .filter(|p| p.as_rule() == Rule::row_group)
        .map(build_row_group)
        .collect()
}

fn build_play_expr(pair: Pair<'_, Rule>) -> Result<PlayExpr, ParseError> {
    // play_expr = { play_term ~ ("," ~ play_term)* }
    let span = span_of(&pair);
    let terms: Result<Vec<PlayTerm>, ParseError> =
        pair.into_inner().map(build_play_term).collect();
    Ok(PlayExpr { terms: terms?, span })
}

fn build_play_term(pair: Pair<'_, Rule>) -> Result<PlayTerm, ParseError> {
    // play_term = { play_atom ~ ("*" ~ nat)? }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let atom = build_play_atom(it.next().unwrap())?;
    let repeat = if let Some(nat_pair) = it.next() {
        let n_span = span_of(&nat_pair);
        let n: u32 = nat_pair.as_str().parse().map_err(|_| ParseError {
            span: n_span,
            message: format!("invalid repeat count: {:?}", nat_pair.as_str()),
        })?;
        if n == 0 {
            return Err(ParseError {
                span: n_span,
                message: "play repeat count must be positive".to_owned(),
            });
        }
        n
    } else {
        1
    };
    Ok(PlayTerm { atom, repeat, span })
}

fn build_play_atom(pair: Pair<'_, Rule>) -> Result<PlayAtom, ParseError> {
    // play_atom = { play_atom_group | ident }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::ident => Ok(PlayAtom::Ref(build_ident(inner))),
        Rule::play_atom_group => {
            let expr_pair = inner.into_inner().next().unwrap();
            Ok(PlayAtom::Group(Box::new(build_play_expr(expr_pair)?)))
        }
        _ => unreachable!("unexpected rule in play_atom: {:?}", inner.as_rule()),
    }
}

fn build_play_body(pair: Pair<'_, Rule>) -> Result<PlayBody, ParseError> {
    // play_body = { inline_block | named_inline | play_expr }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::inline_block => {
            let span = span_of(&inner);
            let row_seq_pair = inner.into_inner().next().unwrap();
            let body = build_row_seq(row_seq_pair)?;
            Ok(PlayBody::Inline { body, span })
        }
        Rule::named_inline => {
            let span = span_of(&inner);
            let mut it = inner.into_inner();
            let name = build_ident(it.next().unwrap());
            let body = build_row_seq(it.next().unwrap())?;
            Ok(PlayBody::NamedInline { name, body, span })
        }
        Rule::play_expr => Ok(PlayBody::Expr(build_play_expr(inner)?)),
        _ => unreachable!("unexpected rule in play_body: {:?}", inner.as_rule()),
    }
}

fn build_song_item(pair: Pair<'_, Rule>) -> Result<SongItem, ParseError> {
    // song_item = { section_def | pattern_block | play_stmt | loop_marker }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::section_def => Ok(SongItem::Section(build_section_def(inner)?)),
        Rule::pattern_block => Ok(SongItem::Pattern(build_pattern_block(inner)?)),
        Rule::play_stmt => {
            let body_pair = inner.into_inner().next().unwrap();
            Ok(SongItem::Play(build_play_body(body_pair)?))
        }
        Rule::loop_marker => Ok(SongItem::LoopMarker(span_of(&inner))),
        _ => unreachable!("unexpected rule in song_item: {:?}", inner.as_rule()),
    }
}

pub(super) fn build_song_block(pair: Pair<'_, Rule>) -> Result<SongDef, ParseError> {
    // song_block = { "song" ~ ident ~ song_lanes ~ "{" ~ song_item* ~ "}" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());

    let lanes_pair = it.next().unwrap();
    let lanes: Vec<Ident> = lanes_pair.into_inner().map(build_ident).collect();

    let items: Result<Vec<SongItem>, ParseError> = it.map(build_song_item).collect();
    Ok(SongDef { name, lanes, items: items?, span })
}
