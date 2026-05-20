//! Runtime tracker data types for pattern sequencing.
//!
//! These types live inside `Arc<TrackerData>` and are read by modules on the
//! audio thread. All structures are optimised for read access: flat arrays,
//! integer indexing, no strings in the hot path.
//!
//! See ADR 0029 for the original design and ADR 0077 for the unified
//! step-event grammar (epic E153) that drives the current shape of
//! [`Step`], [`StepKind`], and [`StepEffect`].

use std::sync::Arc;

/// Surface cell shape carried from the parser into the row-build pass.
///
/// Each [`Step`] cell parses into exactly one variant. The row-build
/// pass [`resolve_step_effects`] consumes the kind (plus channel state)
/// to emit a [`StepEffect`]; the audio thread reads only the effect.
///
/// Variants mirror the seven cell shapes in ADR 0077 § "Surface grammar"
/// plus the rest cell.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum StepKind {
    /// `.` — rest.
    #[default]
    Rest,
    /// `_` — continuation tie. Meaning depends on the channel's open
    /// modifier (sustain / absorbed-roll / slide flow).
    Tie,
    /// Plain triggered note (`value`, `value*N`, `value:cv2`).
    /// `repeat` is the `*N` count (`1` = no roll).
    Note { repeat: u8 },
    /// `value>value` sugar — single-tick slide that snaps to `cv1` at
    /// tick start and lands at the close value at tick boundary.
    /// `cv1_end` / `cv2_end` carry the close values.
    SlideSugar { cv1_end: f32, cv2_end: Option<f32> },
    /// `value>` — opens a multi-tick slide. Triggers on this tick;
    /// close value resolved by the later close cell.
    SlideOpen,
    /// `/value` — sets cv1 (and optionally cv2) without retriggering.
    /// Closes any open slide at the tick boundary.
    StepTo { cv2: Option<f32> },
    /// `>_` — explicit slide-flow / open-from-current. Opens a slide
    /// from the channel's current cv when none is open, otherwise
    /// extends the open slide.
    TieFlow,
    /// `>value[:cv2]` — closes an open slide within this tick (ramp
    /// finishes at `value` at end of tick rather than at the tick
    /// boundary). When `cv2` is `Some`, cv2 also ramps to that
    /// endpoint inside the same tick.
    SlideCloseInTick { cv2: Option<f32> },
}

/// A single step in a pattern channel.
///
/// The audio thread dispatches on [`Step::effect`]; `cv1` / `cv2` /
/// `trigger` / `gate` are carried for diagnostics and module-shell
/// reads but the apply path reads its operands out of the effect
/// variant. `kind` records the surface cell shape so the row-build
/// pass [`resolve_step_effects`] can classify the cell against the
/// channel's open modifiers (slide / roll).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub cv1: f32,
    pub cv2: f32,
    pub trigger: bool,
    pub gate: bool,
    /// Surface cell shape (ADR 0077). Default [`StepKind::Rest`].
    pub kind: StepKind,
    /// Resolved [`StepEffect`] for this cell, produced by
    /// [`resolve_step_effects`]. Default [`StepEffect::Silent`].
    pub effect: StepEffect,
}

impl Default for Step {
    fn default() -> Self {
        Self {
            cv1: 0.0,
            cv2: 0.0,
            trigger: false,
            gate: false,
            kind: StepKind::Rest,
            effect: StepEffect::Silent,
        }
    }
}

/// Resolved per-cell effect produced by [`resolve_step_effects`].
///
/// One variant per resolved semantic (ADR 0077). The pattern player
/// dispatches on this enum exclusively; legacy flag fields on [`Step`]
/// are no longer consulted at runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum StepEffect {
    /// Rest cell `.` — no gate, no trigger, no cv change.
    Silent,
    /// Triggered cell. Snaps cv1/cv2 at the start of the tick and fires
    /// the trigger. Optionally opens a slide (`value>` / `value>value`)
    /// and/or a roll (`value*N`).
    StartNote {
        cv1: f32,
        cv2: f32,
        slide: Option<SlideOpen>,
        roll: Option<RollSpec>,
    },
    /// `/value`: snap cv1 (and optionally cv2) at the tick boundary, no
    /// retrigger. Closes any open slide at boundary.
    StepCv {
        cv1: f32,
        cv2: Option<f32>,
    },
    /// Bare `_` with no active modifier — sustain the gate, hold cv.
    Hold,
    /// `_` absorbed by an open slide, or `>_` continuing an open slide.
    /// The slide's per-channel schedule (set by the slide's owning
    /// effect) continues to drive cv across this tick.
    SlideFlow,
    /// `>_` opening a NEW slide from the channel's current cv (no
    /// trigger). The slide's close target is patched by the later
    /// close cell during row-build.
    OpenSlide { slide: SlideOpen },
    /// `>value[:cv2]`: close an open slide within this tick. Cv1
    /// ramps from the current value to `cv1` across the tick. When
    /// `cv2` is `Some`, cv2 ramps to that endpoint inside the same
    /// tick alongside cv1. No trigger.
    SlideCloseInTick { cv1: f32, cv2: Option<f32> },
    /// Tie cell absorbed by a preceding `*N` roll anchor (E152). The
    /// anchor's in-flight roll schedule owns the channel.
    AbsorbedRoll,
}

/// A slide opened by a `value>` / `value>value` / `>_` cell.
///
/// `close_cv1` is the slide's final cv1 endpoint, resolved by the
/// row-build pass once the close cell is encountered. `close_cv2`
/// mirrors it for cv2 (rare). `span` is the number of ticks the
/// slide schedule covers. `closes_at_boundary` is `true` when the
/// close cell is a `value` or `/value` (slide lands at the tick
/// boundary entering the close cell), `false` when the close cell
/// is `>value` (slide finishes inside the close tick).
#[derive(Debug, Clone, PartialEq)]
pub struct SlideOpen {
    pub close_cv1: f32,
    pub close_cv2: Option<f32>,
    pub span: u8,
    pub closes_at_boundary: bool,
}

/// A roll opened by a `value*N` cell, possibly spread across N+ ticks.
///
/// `count` is the `N` from `*N`; `span` is the tick span across which the
/// N sub-triggers are scheduled (E152: `1 + absorbed-tie count`).
#[derive(Debug, Clone, PartialEq)]
pub struct RollSpec {
    pub count: u8,
    pub span: u8,
}

/// A diagnostic produced by [`resolve_step_effects`].
///
/// Currently emitted for slide cells that never reach a close cell
/// within the channel's step run (ADR 0077 § "Continuation
/// absorption" — every slide-open requires a close).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowBuildError {
    /// Index into the channel's step slice that triggered the error.
    pub cell_index: usize,
    /// Human-readable description (LSP / interpreter wrap with span).
    pub message: String,
}

/// Resolve each cell of a channel's step run into a [`StepEffect`].
///
/// Channel-stateful left-to-right walk (ADR 0077). The pass is the
/// single authoritative resolution of surface cell shape → runtime
/// semantics; the pattern player consumes [`Step::effect`] only.
///
/// Returns row-build diagnostics — currently the indices of
/// slide-open cells that never reach a close cell within the slice.
/// Such slides are degraded to no-op ramps (close to the open cv)
/// so the runtime stays well-defined even when callers ignore the
/// error.
///
/// Idempotent — every cell's effect is reset to [`StepEffect::Silent`]
/// before the walk, so calling twice on the same slice yields the
/// same output and the same error list.
pub fn resolve_step_effects(steps: &mut [Step]) -> Vec<RowBuildError> {
    for s in steps.iter_mut() {
        s.effect = StepEffect::Silent;
    }

    let mut errors = Vec::new();
    let n = steps.len();
    let mut state = ChannelState::Idle;

    for i in 0..n {
        let kind = steps[i].kind;
        let cv1 = steps[i].cv1;
        let cv2 = steps[i].cv2;

        let effect = match kind {
            StepKind::Rest => {
                close_open_slide_no_target(&mut steps[..], &mut state);
                state = ChannelState::Idle;
                StepEffect::Silent
            }
            StepKind::Tie => match state {
                ChannelState::RollActive { head_idx } => {
                    extend_roll_span(&mut steps[head_idx].effect);
                    StepEffect::AbsorbedRoll
                }
                ChannelState::SlideOpen { head_idx } => {
                    extend_slide_span(&mut steps[head_idx].effect);
                    StepEffect::SlideFlow
                }
                ChannelState::Idle => StepEffect::Hold,
            },
            StepKind::TieFlow => match state {
                ChannelState::SlideOpen { head_idx } => {
                    extend_slide_span(&mut steps[head_idx].effect);
                    StepEffect::SlideFlow
                }
                ChannelState::Idle | ChannelState::RollActive { .. } => {
                    state = ChannelState::SlideOpen { head_idx: i };
                    StepEffect::OpenSlide {
                        slide: SlideOpen {
                            close_cv1: 0.0,
                            close_cv2: None,
                            span: 1,
                            closes_at_boundary: true,
                        },
                    }
                }
            },
            StepKind::Note { repeat } => {
                if let ChannelState::SlideOpen { head_idx } = state {
                    patch_slide_close(
                        &mut steps[head_idx].effect,
                        cv1,
                        None,
                        true,
                    );
                }
                let roll = if repeat > 1 {
                    Some(RollSpec { count: repeat, span: 1 })
                } else {
                    None
                };
                state = if roll.is_some() {
                    ChannelState::RollActive { head_idx: i }
                } else {
                    ChannelState::Idle
                };
                StepEffect::StartNote { cv1, cv2, slide: None, roll }
            }
            StepKind::SlideSugar { cv1_end, cv2_end } => {
                // value>value — self-closing one-tick slide. Closes any
                // prior open slide at boundary with this cell's cv1,
                // then opens *and closes* its own slide in a single
                // cell (close target already resolved by the sugar).
                // Channel state returns to idle.
                if let ChannelState::SlideOpen { head_idx } = state {
                    patch_slide_close(
                        &mut steps[head_idx].effect,
                        cv1,
                        None,
                        true,
                    );
                }
                state = ChannelState::Idle;
                StepEffect::StartNote {
                    cv1,
                    cv2,
                    slide: Some(SlideOpen {
                        close_cv1: cv1_end,
                        close_cv2: cv2_end,
                        span: 1,
                        closes_at_boundary: true,
                    }),
                    roll: None,
                }
            }
            StepKind::SlideOpen => {
                // value> — closes any prior open slide at boundary,
                // then opens a new multi-tick slide. Close target
                // resolved by the later close cell.
                if let ChannelState::SlideOpen { head_idx } = state {
                    patch_slide_close(
                        &mut steps[head_idx].effect,
                        cv1,
                        None,
                        true,
                    );
                }
                state = ChannelState::SlideOpen { head_idx: i };
                StepEffect::StartNote {
                    cv1,
                    cv2,
                    slide: Some(SlideOpen {
                        close_cv1: 0.0,
                        close_cv2: None,
                        span: 1,
                        closes_at_boundary: true,
                    }),
                    roll: None,
                }
            }
            StepKind::StepTo { cv2: cv2_opt } => {
                if let ChannelState::SlideOpen { head_idx } = state {
                    patch_slide_close(
                        &mut steps[head_idx].effect,
                        cv1,
                        cv2_opt,
                        true,
                    );
                }
                state = ChannelState::Idle;
                StepEffect::StepCv { cv1, cv2: cv2_opt }
            }
            StepKind::SlideCloseInTick { cv2: cv2_opt } => {
                if let ChannelState::SlideOpen { head_idx } = state {
                    // The close cell is itself a slide-ramp tick; bump
                    // span by one before flagging closes_at_boundary
                    // = false so the schedule covers this tick too.
                    extend_slide_span(&mut steps[head_idx].effect);
                    patch_slide_close(
                        &mut steps[head_idx].effect,
                        cv1,
                        cv2_opt,
                        false,
                    );
                } else {
                    errors.push(RowBuildError {
                        cell_index: i,
                        message: "`>value` close cell without a preceding \
                                  slide-open"
                            .to_owned(),
                    });
                }
                state = ChannelState::Idle;
                StepEffect::SlideCloseInTick { cv1, cv2: cv2_opt }
            }
        };

        steps[i].effect = effect;
    }

    // Any slide still open at end-of-run is an error: the close cell
    // never arrived. Degrade to a no-op (ramp to current cv) so the
    // runtime stays sane; record the diagnostic.
    if let ChannelState::SlideOpen { head_idx } = state {
        errors.push(RowBuildError {
            cell_index: head_idx,
            message: "slide opened by this cell never reaches a close cell \
                      (`value`, `/value`, `>value`) within the channel's \
                      step run"
                .to_owned(),
        });
        let open_cv1 = steps[head_idx].cv1;
        patch_slide_close(&mut steps[head_idx].effect, open_cv1, None, true);
    }

    errors
}

#[derive(Debug, Clone, Copy)]
enum ChannelState {
    Idle,
    SlideOpen { head_idx: usize },
    RollActive { head_idx: usize },
}

fn extend_slide_span(effect: &mut StepEffect) {
    if let Some(so) = slide_in_effect(effect) {
        so.span = so.span.saturating_add(1);
    }
}

fn extend_roll_span(effect: &mut StepEffect) {
    if let StepEffect::StartNote { roll: Some(r), .. } = effect {
        r.span = r.span.saturating_add(1);
    }
}

fn patch_slide_close(
    effect: &mut StepEffect,
    close_cv1: f32,
    close_cv2: Option<f32>,
    at_boundary: bool,
) {
    if let Some(so) = slide_in_effect(effect) {
        so.close_cv1 = close_cv1;
        if close_cv2.is_some() {
            so.close_cv2 = close_cv2;
        }
        so.closes_at_boundary = at_boundary;
    }
}

fn slide_in_effect(effect: &mut StepEffect) -> Option<&mut SlideOpen> {
    match effect {
        StepEffect::StartNote { slide: Some(so), .. } => Some(so),
        StepEffect::OpenSlide { slide: so } => Some(so),
        _ => None,
    }
}

fn close_open_slide_no_target(steps: &mut [Step], state: &mut ChannelState) {
    // Rest cells are not legal close cells (no value); leave the
    // SlideOpen pointed at its current placeholder. The end-of-run
    // sweep emits the error.
    let _ = steps;
    let _ = state;
}

/// A multi-channel grid of step data.
///
/// Indexed by `[channel][step]`. The channel count and step count are stored
/// explicitly for bounds checking.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    /// Number of channels in this pattern.
    pub channels: usize,
    /// Number of steps per channel.
    pub steps: usize,
    /// Step data indexed as `[channel][step]`.
    pub data: Vec<Vec<Step>>,
}

/// A collection of patterns indexed by bank position.
///
/// Patterns are assigned bank indices by alphabetical sort on their names.
/// The name-to-index mapping is resolved at interpret time and encoded into
/// the `Song` order table.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternBank {
    pub patterns: Vec<Pattern>,
}

/// A song arrangement — which patterns play in which order across channels.
#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    /// Number of song-level channels.
    pub channels: usize,
    /// Order table: `[row][channel]` → pattern bank index.
    /// `None` indicates silence on that channel for that row.
    pub order: Vec<Vec<Option<usize>>>,
    /// Row index to loop back to (0 if no `@loop` annotation).
    pub loop_point: usize,
}

/// A collection of songs, indexed by position.
///
/// Songs are stored in a flat `Vec` for O(1) access by index on the audio
/// thread. Name-to-index resolution is the interpreter's concern and does
/// **not** live inside `TrackerData`: no strings cross into the audio thread.
#[derive(Debug, Clone, PartialEq)]
pub struct SongBank {
    pub songs: Vec<Song>,
}

/// All pattern and song data for a patch, shared via `Arc`.
///
/// Distributed to modules at plan activation. The audio thread reads through
/// the `Arc` — no atomics, no contention on the read path.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackerData {
    pub patterns: PatternBank,
    pub songs: SongBank,
}

/// Opt-in trait for modules that receive tracker data (patterns and songs).
///
/// Modules that want tracker data implement this trait and override
/// [`Module::as_tracker_data_receiver`](crate::Module::as_tracker_data_receiver)
/// to return `Some(self)`. Modules that do not implement this trait pay zero
/// cost — the planner ignores them.
///
/// Called once per plan activation with `Arc::clone` (ref-count bump only).
/// Implementations must not allocate, block, or perform I/O.
pub trait ReceivesTrackerData {
    /// Receive tracker data at plan activation.
    ///
    /// The `Arc` is cloned (ref-count bump) once per module. The audio thread's
    /// read path is plain pointer dereference through the `Arc`.
    fn receive_tracker_data(&mut self, data: Arc<TrackerData>);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_data_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TrackerData>();
        assert_send_sync::<Arc<TrackerData>>();
    }

    #[test]
    fn empty_tracker_data() {
        let data = TrackerData {
            patterns: PatternBank { patterns: vec![] },
            songs: SongBank { songs: vec![] },
        };
        assert_eq!(data.patterns.patterns.len(), 0);
        assert_eq!(data.songs.songs.len(), 0);
    }

    fn note(cv1: f32) -> Step {
        Step {
            cv1,
            trigger: true,
            gate: true,
            kind: StepKind::Note { repeat: 1 },
            ..Step::default()
        }
    }

    fn roll(cv1: f32, repeat: u8) -> Step {
        Step {
            cv1,
            trigger: true,
            gate: true,
            kind: StepKind::Note { repeat },
            ..Step::default()
        }
    }

    fn tie() -> Step {
        Step { gate: true, kind: StepKind::Tie, ..Step::default() }
    }

    fn rest() -> Step {
        Step::default()
    }

    fn slide_sugar(cv1: f32, cv1_end: f32) -> Step {
        Step {
            cv1,
            trigger: true,
            gate: true,
            kind: StepKind::SlideSugar { cv1_end, cv2_end: None },
            ..Step::default()
        }
    }

    fn slide_open(cv1: f32) -> Step {
        Step {
            cv1,
            trigger: true,
            gate: true,
            kind: StepKind::SlideOpen,
            ..Step::default()
        }
    }

    fn step_to(cv1: f32) -> Step {
        Step {
            cv1,
            gate: true,
            kind: StepKind::StepTo { cv2: None },
            ..Step::default()
        }
    }

    fn tie_flow() -> Step {
        Step { gate: true, kind: StepKind::TieFlow, ..Step::default() }
    }

    fn slide_close_in_tick(cv1: f32) -> Step {
        Step {
            cv1,
            gate: true,
            kind: StepKind::SlideCloseInTick { cv2: None },
            ..Step::default()
        }
    }

    fn slide_close_in_tick_with_cv2(cv1: f32, cv2: f32) -> Step {
        Step {
            cv1,
            gate: true,
            kind: StepKind::SlideCloseInTick { cv2: Some(cv2) },
            ..Step::default()
        }
    }

    // ── Smoke / dataflow ──────────────────────────────────────────────────

    #[test]
    fn pattern_bank_indexing() {
        let step = note(1.0);
        let pattern = Pattern {
            channels: 1,
            steps: 2,
            data: vec![vec![step.clone(), step]],
        };
        let bank = PatternBank { patterns: vec![pattern] };
        assert_eq!(bank.patterns[0].channels, 1);
        assert_eq!(bank.patterns[0].steps, 2);
        assert_eq!(bank.patterns[0].data[0].len(), 2);
    }

    #[test]
    fn song_order_and_loop_point() {
        let song = Song {
            channels: 2,
            order: vec![
                vec![Some(0), Some(1)],
                vec![Some(0), Some(1)],
            ],
            loop_point: 1,
        };
        assert_eq!(song.order.len(), 2);
        assert_eq!(song.loop_point, 1);
        assert_eq!(song.order[0][0], Some(0));
    }

    #[test]
    fn song_silence_entries() {
        let song = Song {
            channels: 2,
            order: vec![vec![Some(0), None]],
            loop_point: 0,
        };
        assert_eq!(song.order[0][1], None);
    }

    // ── E153 (ticket 0946): StepKind → StepEffect resolution ────────────

    #[test]
    fn resolve_value_cell_is_start_note() {
        let mut steps = vec![note(60.0)];
        let errs = resolve_step_effects(&mut steps);
        assert!(errs.is_empty());
        match &steps[0].effect {
            StepEffect::StartNote { cv1, slide, roll, .. } => {
                assert_eq!(*cv1, 60.0);
                assert!(slide.is_none());
                assert!(roll.is_none());
            }
            other => panic!("expected StartNote, got {other:?}"),
        }
    }

    #[test]
    fn resolve_value_x_n_span_1_is_start_note_with_roll() {
        let mut steps = vec![roll(60.0, 3)];
        let _ = resolve_step_effects(&mut steps);
        match &steps[0].effect {
            StepEffect::StartNote { roll: Some(r), slide, .. } => {
                assert_eq!(r.count, 3);
                assert_eq!(r.span, 1);
                assert!(slide.is_none());
            }
            other => panic!("expected StartNote+roll, got {other:?}"),
        }
    }

    #[test]
    fn resolve_value_x_n_underscore_underscore_absorbs_ties() {
        // `x*3 _ _` — anchor span=3, two absorbed ties.
        let mut steps = vec![roll(60.0, 3), tie(), tie()];
        let _ = resolve_step_effects(&mut steps);
        match &steps[0].effect {
            StepEffect::StartNote { roll: Some(r), .. } => {
                assert_eq!(r.count, 3);
                assert_eq!(r.span, 3);
            }
            other => panic!("expected StartNote+roll(span=3), got {other:?}"),
        }
        assert!(matches!(steps[1].effect, StepEffect::AbsorbedRoll));
        assert!(matches!(steps[2].effect, StepEffect::AbsorbedRoll));
    }

    #[test]
    fn resolve_tie_after_note_is_hold() {
        let mut steps = vec![note(60.0), tie()];
        let _ = resolve_step_effects(&mut steps);
        assert!(matches!(steps[0].effect, StepEffect::StartNote { .. }));
        assert!(matches!(steps[1].effect, StepEffect::Hold));
    }

    #[test]
    fn resolve_rest_is_silent() {
        let mut steps = vec![rest()];
        let _ = resolve_step_effects(&mut steps);
        assert!(matches!(steps[0].effect, StepEffect::Silent));
    }

    #[test]
    fn resolve_slide_sugar_one_tick() {
        // value>value — span 1, closes at boundary.
        let mut steps = vec![slide_sugar(60.0, 72.0)];
        let errs = resolve_step_effects(&mut steps);
        assert!(errs.is_empty());
        match &steps[0].effect {
            StepEffect::StartNote { slide: Some(so), cv1, .. } => {
                assert_eq!(*cv1, 60.0);
                assert_eq!(so.close_cv1, 72.0);
                assert_eq!(so.span, 1);
                assert!(so.closes_at_boundary);
            }
            other => panic!("expected StartNote+slide, got {other:?}"),
        }
    }

    // ── ADR 0077 §"Continuation absorption" worked examples ─────────────

    #[test]
    fn adr_example_e4_underscore_tieflow_step_to_g4() {
        // E4 _ >_ /G4
        // Tick 1: StartNote(E4) (no slide)
        // Tick 2: Hold (E4 sustains)
        // Tick 3: OpenSlide{close=G4, span=1, closes_at_boundary=true}
        // Tick 4: StepCv(G4) — closes slide at boundary, holds.
        let mut steps = vec![note(60.0), tie(), tie_flow(), step_to(67.0)];
        let errs = resolve_step_effects(&mut steps);
        assert!(errs.is_empty(), "no errors: {errs:?}");
        assert!(matches!(steps[0].effect, StepEffect::StartNote { .. }));
        assert!(matches!(steps[1].effect, StepEffect::Hold));
        match &steps[2].effect {
            StepEffect::OpenSlide { slide: so } => {
                assert_eq!(so.close_cv1, 67.0);
                assert_eq!(so.span, 1);
                assert!(so.closes_at_boundary);
            }
            other => panic!("expected OpenSlide, got {other:?}"),
        }
        assert!(matches!(steps[3].effect, StepEffect::StepCv { cv1: 67.0, .. }));
    }

    #[test]
    fn adr_example_e4_open_underscore_step_to_g4() {
        // E4> _ /G4
        // Tick 1: StartNote(E4) + SlideOpen{close=G4, span=2, boundary=true}
        // Tick 2: SlideFlow
        // Tick 3: StepCv(G4)
        let mut steps = vec![slide_open(60.0), tie(), step_to(67.0)];
        let _ = resolve_step_effects(&mut steps);
        match &steps[0].effect {
            StepEffect::StartNote { slide: Some(so), .. } => {
                assert_eq!(so.close_cv1, 67.0);
                assert_eq!(so.span, 2);
                assert!(so.closes_at_boundary);
            }
            other => panic!("expected StartNote+slide, got {other:?}"),
        }
        assert!(matches!(steps[1].effect, StepEffect::SlideFlow));
        assert!(matches!(steps[2].effect, StepEffect::StepCv { cv1: 67.0, .. }));
    }

    #[test]
    fn adr_example_e4_open_underscore_close_in_tick_g4() {
        // E4> _ >G4
        // Tick 1: StartNote(E4) + SlideOpen{close=G4, span=3, boundary=false}
        // Tick 2: SlideFlow
        // Tick 3: SlideCloseInTick(G4)
        let mut steps = vec![slide_open(60.0), tie(), slide_close_in_tick(67.0)];
        let _ = resolve_step_effects(&mut steps);
        match &steps[0].effect {
            StepEffect::StartNote { slide: Some(so), .. } => {
                assert_eq!(so.close_cv1, 67.0);
                assert_eq!(so.span, 3);
                assert!(!so.closes_at_boundary);
            }
            other => panic!("expected StartNote+slide, got {other:?}"),
        }
        assert!(matches!(steps[1].effect, StepEffect::SlideFlow));
        assert!(matches!(
            steps[2].effect,
            StepEffect::SlideCloseInTick { cv1: 67.0, .. }
        ));
    }

    #[test]
    fn adr_example_e4_open_underscore_note_g4() {
        // E4> _ G4
        // Tick 1: StartNote(E4) + SlideOpen{close=G4, span=2, boundary=true}
        // Tick 2: SlideFlow
        // Tick 3: StartNote(G4) (fresh trigger, slide already closed at
        //         tick boundary entering this cell)
        let mut steps = vec![slide_open(60.0), tie(), note(67.0)];
        let _ = resolve_step_effects(&mut steps);
        match &steps[0].effect {
            StepEffect::StartNote { slide: Some(so), .. } => {
                assert_eq!(so.close_cv1, 67.0);
                assert_eq!(so.span, 2);
                assert!(so.closes_at_boundary);
            }
            other => panic!("expected StartNote+slide, got {other:?}"),
        }
        assert!(matches!(steps[1].effect, StepEffect::SlideFlow));
        match &steps[2].effect {
            StepEffect::StartNote { cv1, slide, .. } => {
                assert_eq!(*cv1, 67.0);
                assert!(slide.is_none());
            }
            other => panic!("expected StartNote, got {other:?}"),
        }
    }

    #[test]
    fn adr_example_e4_step_to_g4() {
        // E4 /G4 — bare note then StepCv.
        let mut steps = vec![note(60.0), step_to(67.0)];
        let _ = resolve_step_effects(&mut steps);
        match &steps[0].effect {
            StepEffect::StartNote { cv1, slide, .. } => {
                assert_eq!(*cv1, 60.0);
                assert!(slide.is_none());
            }
            other => panic!("expected StartNote, got {other:?}"),
        }
        assert!(matches!(
            steps[1].effect,
            StepEffect::StepCv { cv1: 67.0, .. }
        ));
    }

    #[test]
    fn value_close_value_sugar_equivalent_to_value_open_step_to_value() {
        // C4>E4 _   ≡   C4> /E4
        // The sugar form is two cells: SlideSugar + Tie (SlideFlow).
        // The expanded form is two cells: SlideOpen + StepTo.
        // Audibly the cv1 trajectory matches: ramp C4→E4 over tick 1, hold
        // E4 on tick 2. Trigger only on tick 1.
        let mut sugar = vec![slide_sugar(60.0, 64.0), tie()];
        let mut expanded = vec![slide_open(60.0), step_to(64.0)];
        let _ = resolve_step_effects(&mut sugar);
        let _ = resolve_step_effects(&mut expanded);

        let sugar_slide = match &sugar[0].effect {
            StepEffect::StartNote { slide: Some(so), .. } => so.clone(),
            other => panic!("sugar tick 0: {other:?}"),
        };
        let expanded_slide = match &expanded[0].effect {
            StepEffect::StartNote { slide: Some(so), .. } => so.clone(),
            other => panic!("expanded tick 0: {other:?}"),
        };
        assert_eq!(sugar_slide, expanded_slide);
        // sugar's slide is self-closing, so the trailing tie is a plain
        // Hold; the expanded form's second cell is the StepCv close.
        // Audibly equivalent: both shapes ramp 60→64 over tick 1 and
        // sustain 64 across tick 2.
        assert!(matches!(sugar[1].effect, StepEffect::Hold));
        assert!(matches!(
            expanded[1].effect,
            StepEffect::StepCv { cv1: 64.0, .. }
        ));
    }

    #[test]
    fn slide_open_without_close_emits_error_and_degrades() {
        let mut steps = vec![slide_open(60.0), tie()];
        let errs = resolve_step_effects(&mut steps);
        assert_eq!(errs.len(), 1, "expected one row-build error: {errs:?}");
        assert_eq!(errs[0].cell_index, 0);
        // Degraded: close = open cv1 (no audible ramp).
        match &steps[0].effect {
            StepEffect::StartNote { slide: Some(so), .. } => {
                assert_eq!(so.close_cv1, 60.0);
            }
            other => panic!("expected degraded slide, got {other:?}"),
        }
    }

    #[test]
    fn slide_close_in_tick_without_open_emits_error() {
        let mut steps = vec![slide_close_in_tick(67.0)];
        let errs = resolve_step_effects(&mut steps);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].cell_index, 0);
    }

    // ── Ticket 0948: cv2 on multi-cell slides ──────────────────────────

    fn slide_open_with_cv2(cv1: f32, cv2: f32) -> Step {
        Step {
            cv1,
            cv2,
            trigger: true,
            gate: true,
            kind: StepKind::SlideOpen,
            ..Step::default()
        }
    }

    fn step_to_with_cv2(cv1: f32, cv2: f32) -> Step {
        Step {
            cv1,
            gate: true,
            kind: StepKind::StepTo { cv2: Some(cv2) },
            ..Step::default()
        }
    }

    #[test]
    fn slide_open_with_cv2_then_close_in_tick_with_cv2() {
        // `A4:0.5> >B4:0.8` — open cv2=0.5, close cv2=0.8 inside tick 2.
        let mut steps = vec![
            slide_open_with_cv2(60.0, 0.5),
            slide_close_in_tick_with_cv2(72.0, 0.8),
        ];
        let errs = resolve_step_effects(&mut steps);
        assert!(errs.is_empty());
        match &steps[0].effect {
            StepEffect::StartNote { cv1, cv2, slide: Some(so), .. } => {
                assert_eq!(*cv1, 60.0);
                assert_eq!(*cv2, 0.5);
                assert_eq!(so.close_cv1, 72.0);
                assert_eq!(so.close_cv2, Some(0.8));
                assert_eq!(so.span, 2);
                assert!(!so.closes_at_boundary);
            }
            other => panic!("expected StartNote+slide, got {other:?}"),
        }
    }

    #[test]
    fn slide_open_with_cv2_then_step_to_with_cv2() {
        // `A4:0.5> /B4:0.8` — open cv2=0.5, boundary close cv2=0.8.
        let mut steps = vec![
            slide_open_with_cv2(60.0, 0.5),
            step_to_with_cv2(72.0, 0.8),
        ];
        let errs = resolve_step_effects(&mut steps);
        assert!(errs.is_empty());
        match &steps[0].effect {
            StepEffect::StartNote { slide: Some(so), .. } => {
                assert_eq!(so.close_cv1, 72.0);
                assert_eq!(so.close_cv2, Some(0.8));
                assert_eq!(so.span, 1);
                assert!(so.closes_at_boundary);
            }
            other => panic!("expected StartNote+slide, got {other:?}"),
        }
        match &steps[1].effect {
            StepEffect::StepCv { cv1, cv2 } => {
                assert_eq!(*cv1, 72.0);
                assert_eq!(*cv2, Some(0.8));
            }
            other => panic!("expected StepCv, got {other:?}"),
        }
    }

    #[test]
    fn slide_open_with_cv2_then_close_without_cv2_leaves_close_cv2_none() {
        // `A4:0.5> >B4` — open carries cv2; close has no `:cv2`.
        // `close_cv2` stays `None`; runtime falls back to open's cv2.
        let mut steps = vec![
            slide_open_with_cv2(60.0, 0.5),
            slide_close_in_tick(72.0),
        ];
        let _ = resolve_step_effects(&mut steps);
        match &steps[0].effect {
            StepEffect::StartNote { slide: Some(so), .. } => {
                assert_eq!(so.close_cv1, 72.0);
                assert_eq!(so.close_cv2, None);
            }
            other => panic!("expected StartNote+slide, got {other:?}"),
        }
    }

    #[test]
    fn resolve_is_idempotent() {
        let mut steps = vec![
            note(60.0),
            slide_open(60.0),
            tie(),
            slide_close_in_tick(72.0),
            rest(),
            roll(48.0, 3),
            tie(),
            tie(),
        ];
        let _ = resolve_step_effects(&mut steps);
        let snapshot = steps.clone();
        let _ = resolve_step_effects(&mut steps);
        assert_eq!(steps, snapshot);
    }
}
