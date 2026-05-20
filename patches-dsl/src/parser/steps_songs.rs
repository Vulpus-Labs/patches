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

/// Parse a cv1 value from a step primary pair (step_note, step_trigger, step_float, step_int, step_unit).
fn parse_cv1_value(pair: &Pair<'_, Rule>) -> Result<f32, ParseError> {
    let span = span_of(pair);
    match pair.as_rule() {
        Rule::step_note => parse_step_note(pair.as_str(), span),
        Rule::step_trigger => Ok(0.0),
        Rule::step_float | Rule::step_int => parse_step_float(pair.as_str(), span),
        Rule::step_unit => parse_step_unit(pair.as_str(), span),
        _ => unreachable!("unexpected cv1 rule: {:?}", pair.as_rule()),
    }
}

/// Parse a slide target value from a step_slide_target's inner pair.
fn parse_slide_target_value(pair: Pair<'_, Rule>) -> Result<f32, ParseError> {
    let inner = pair.into_inner().next().unwrap();
    let span = span_of(&inner);
    match inner.as_rule() {
        Rule::step_note => parse_step_note(inner.as_str(), span),
        Rule::step_float | Rule::step_int => parse_step_float(inner.as_str(), span),
        Rule::step_unit => parse_step_unit(inner.as_str(), span),
        _ => unreachable!("unexpected slide target rule: {:?}", inner.as_rule()),
    }
}

/// Parse the `:value` cv2 tail on a slide cell (`step_cv2_tail`).
/// Inner pair is `step_unit | step_float | step_int`.
fn parse_cv2_tail(pair: Pair<'_, Rule>) -> Result<f32, ParseError> {
    let inner = pair.into_inner().next().unwrap();
    let span = span_of(&inner);
    match inner.as_rule() {
        Rule::step_float | Rule::step_int => parse_step_float(inner.as_str(), span),
        Rule::step_unit => parse_step_unit(inner.as_str(), span),
        _ => unreachable!("unexpected cv2 tail rule: {:?}", inner.as_rule()),
    }
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
            // step_step_to = ${ "/" ~ primary ~ step_cv2_tail? }
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
                kind: StepKind::StepTo { cv2 },
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
            let mut it = inner.into_inner();
            // First child: the cv1 primary value
            let cv1_pair = it.next().unwrap();
            let cv1 = parse_cv1_value(&cv1_pair)?;
            let mut cv1_end: Option<f32> = None;
            let mut cv2: f32 = 0.0;
            let mut cv2_end: Option<f32> = None;
            let mut repeat: u8 = 1;

            for child in it {
                match child.as_rule() {
                    Rule::step_slide_target => {
                        cv1_end = Some(parse_slide_target_value(child)?);
                    }
                    Rule::step_cv2 => {
                        let mut cv2_it = child.into_inner();
                        let cv2_val_pair = cv2_it.next().unwrap();
                        let cv2_span = span_of(&cv2_val_pair);
                        cv2 = match cv2_val_pair.as_rule() {
                            Rule::step_float | Rule::step_int => {
                                parse_step_float(cv2_val_pair.as_str(), cv2_span)?
                            }
                            Rule::step_unit => {
                                parse_step_unit(cv2_val_pair.as_str(), cv2_span)?
                            }
                            _ => unreachable!(
                                "unexpected cv2 rule: {:?}",
                                cv2_val_pair.as_rule()
                            ),
                        };
                        // Optional cv2 slide target
                        if let Some(slide_pair) = cv2_it.next() {
                            cv2_end = Some(parse_slide_target_value(slide_pair)?);
                        }
                    }
                    Rule::step_repeat => {
                        // step_repeat = ${ "*" ~ nat }
                        let nat_pair = child.into_inner().next().unwrap();
                        let span = span_of(&nat_pair);
                        repeat = nat_pair.as_str().parse::<u8>().map_err(|_| ParseError {
                            span,
                            message: format!("invalid repeat count: {:?}", nat_pair.as_str()),
                        })?;
                    }
                    _ => unreachable!("unexpected rule in step_valued: {:?}", child.as_rule()),
                }
            }

            // Classify: `value>value` sugar (slide-target present) is
            // SlideSugar; otherwise it's a Note (with optional `*N` /
            // `:cv2`). `value*N` with a slide target is rejected by the
            // grammar (step_slide_target precedes step_repeat in the
            // chain, but the grammar allows them — defensively reject
            // mixing slide-target + roll here as the semantics are
            // undefined).
            let kind = if let Some(end) = cv1_end {
                if repeat > 1 {
                    return Err(ParseError {
                        span: span_of(&cv1_pair),
                        message: "slide sugar `value>value` cannot also \
                                  carry a `*N` repeat suffix"
                            .to_owned(),
                    });
                }
                StepKind::SlideSugar { cv1_end: end, cv2_end }
            } else {
                StepKind::Note { repeat }
            };

            Ok(Step {
                cv1,
                cv2,
                trigger: true,
                gate: true,
                kind,
            })
        }
        _ => unreachable!("unexpected rule in step: {:?}", inner.as_rule()),
    }
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
