//! Runtime tracker data types for pattern sequencing.
//!
//! These types live inside `Arc<TrackerData>` and are read by modules on the
//! audio thread. All structures are optimised for read access: flat arrays,
//! integer indexing, no strings in the hot path.
//!
//! See ADR 0029 for the full design.

use std::sync::Arc;

/// A single step in a pattern channel.
///
/// Every step produces four values: `cv1`, `cv2`, `trigger`, and `gate`.
/// Optional `cv1_end` / `cv2_end` specify slide targets; `repeat` subdivides
/// the tick into multiple evenly-spaced triggers.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub cv1: f32,
    pub cv2: f32,
    pub trigger: bool,
    pub gate: bool,
    /// Slide target for cv1 (interpolates from `cv1` to `cv1_end` over the tick).
    pub cv1_end: Option<f32>,
    /// Slide target for cv2 (interpolates from `cv2` to `cv2_end` over the tick).
    pub cv2_end: Option<f32>,
    /// Repeat count: 1 = normal, >1 = subdivide the tick into `repeat` triggers.
    pub repeat: u8,
    /// Tick span across which a `*N` roll is spread (epic E152). For an
    /// anchor with `repeat > 1` followed by `k` consecutive tie cells
    /// absorbed by the roll, `repeat_span = 1 + k`. Default `1` (no
    /// spread). Always `1` when `repeat == 1`.
    pub repeat_span: u8,
    /// True if this tie cell has been absorbed by a preceding `*N` roll
    /// anchor. The pattern player suppresses the cell's normal tie
    /// handling and lets the anchor's in-flight schedule run through.
    /// Default `false`.
    pub absorbed_by_roll: bool,
    /// Resolved [`StepEffect`] for this cell (ADR 0077, epic E153). Set
    /// by [`resolve_step_effects`]. Default [`StepEffect::Silent`].
    ///
    /// Additive alongside the legacy `trigger` / `gate` / `cv1_end` /
    /// `repeat` / `repeat_span` / `absorbed_by_roll` flags until ticket
    /// 0944 flips the pattern player onto the effect-based dispatch.
    pub effect: StepEffect,
}

impl Default for Step {
    fn default() -> Self {
        Self {
            cv1: 0.0,
            cv2: 0.0,
            trigger: false,
            gate: false,
            cv1_end: None,
            cv2_end: None,
            repeat: 1,
            repeat_span: 1,
            absorbed_by_roll: false,
            effect: StepEffect::Silent,
        }
    }
}

/// Resolved per-cell effect produced by [`resolve_step_effects`].
///
/// One variant per surface cell shape (ADR 0077). The pattern player
/// dispatches on this enum instead of inspecting the legacy
/// `trigger` / `gate` / `cv1_end` / `repeat` / `absorbed_by_roll` flag
/// soup (ticket 0944).
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
    /// retrigger. Closes any open slide.
    StepCv {
        cv1: f32,
        cv2: Option<f32>,
    },
    /// Bare `_` with no active modifier — sustain the gate, hold cv.
    Hold,
    /// `_` absorbed by an open slide, or explicit `>_`. The slide's
    /// per-channel schedule (set by the prior [`SlideOpen`]) continues
    /// to drive cv across this tick.
    SlideFlow,
    /// `>value`: close an open slide within this tick. Cv1 ramps from
    /// the current value to `cv1` across the tick. No trigger.
    SlideCloseInTick { cv1: f32 },
    /// Tie cell absorbed by a preceding `*N` roll anchor (E152). The
    /// anchor's in-flight roll schedule owns the channel.
    AbsorbedRoll,
}

/// A slide opened by a triggered cell (`value>` / `value>value`).
///
/// `close_cv1` is the slide's final cv1 endpoint, resolved by the
/// row-build pass once the close cell is encountered. `closes_at_boundary`
/// is `true` when the close cell is a `value` or `/value` (slide lands at
/// the tick boundary entering the close cell), `false` when the close is
/// `>value` (slide finishes inside the close tick).
#[derive(Debug, Clone, PartialEq)]
pub struct SlideOpen {
    pub close_cv1: f32,
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

/// Annotate `*N` roll anchors in a channel's step sequence with the
/// number of consecutive following tie cells they absorb, and mark each
/// absorbed tie cell with `absorbed_by_roll = true`.
///
/// A tie is a step with `gate == true && trigger == false`. Only
/// anchors with `repeat > 1` absorb ties; ordinary steps and ties keep
/// `repeat_span = 1` and `absorbed_by_roll = false`. The scan never
/// crosses an already-absorbed cell, so back-to-back `*N` anchors split
/// their tie runs cleanly.
///
/// Idempotent — resets `repeat_span` to `1` and clears
/// `absorbed_by_roll` before scanning, so calling twice on the same
/// slice yields the same result.
pub fn annotate_repeat_spans(steps: &mut [Step]) {
    for s in steps.iter_mut() {
        s.repeat_span = 1;
        s.absorbed_by_roll = false;
    }
    let n = steps.len();
    let mut i = 0;
    while i < n {
        if steps[i].repeat > 1 {
            let mut j = i + 1;
            while j < n && steps[j].gate && !steps[j].trigger && steps[j].repeat <= 1 {
                steps[j].absorbed_by_roll = true;
                j += 1;
            }
            steps[i].repeat_span = (j - i) as u8;
            i = j;
        } else {
            i += 1;
        }
    }
}

/// Resolve each cell of a channel's step run into a [`StepEffect`].
///
/// Channel-stateful left-to-right walk (ADR 0077, ticket 0943). The pass
/// is the single authoritative resolution of surface cell shape →
/// runtime semantics; the pattern player consumes [`Step::effect`] and
/// never inspects the legacy flag soup.
///
/// Idempotent — every cell's effect is reset to [`StepEffect::Silent`]
/// before the walk, so calling twice on the same slice yields the same
/// output. Requires `annotate_repeat_spans` to have run first so that
/// `absorbed_by_roll` is set on the tie cells absorbed by a preceding
/// `*N` anchor.
///
/// Surface forms recognised today (ticket 0946 extends this for the
/// new `_` / `/value` / `>value` / `>_` / `value>` grammar):
///
/// - Rest (`!gate && !trigger`) → [`StepEffect::Silent`].
/// - Roll anchor (`trigger && repeat > 1`) → [`StepEffect::StartNote`]
///   with `roll = Some(RollSpec)`. Span from `repeat_span` (E152).
/// - Slide head (`trigger && cv1_end.is_some()`) → [`StepEffect::StartNote`]
///   with `slide = Some(SlideOpen { close_cv1, closes_at_boundary: true })`.
///   `close_cv1` is patched as later tied-slide cells extend the run.
/// - Plain triggered note (`trigger && !cv1_end && repeat == 1`) →
///   [`StepEffect::StartNote`] with both `slide` and `roll` `None`.
/// - Absorbed tie (`absorbed_by_roll`) → [`StepEffect::AbsorbedRoll`].
/// - Tie carrying a slide target after a slide head (`!trigger && gate
///   && cv1_end.is_some()`) → [`StepEffect::SlideFlow`], and the head's
///   `close_cv1` is updated to this cell's `cv1_end`.
/// - Bare tie after a non-slide note (`!trigger && gate`) →
///   [`StepEffect::Hold`].
pub fn resolve_step_effects(steps: &mut [Step]) {
    for s in steps.iter_mut() {
        s.effect = StepEffect::Silent;
    }

    let n = steps.len();
    let mut slide_head: Option<usize> = None;
    let mut i = 0;
    while i < n {
        let cv1 = steps[i].cv1;
        let cv2 = steps[i].cv2;
        let trigger = steps[i].trigger;
        let gate = steps[i].gate;
        let cv1_end = steps[i].cv1_end;
        let repeat = steps[i].repeat;
        let repeat_span = steps[i].repeat_span.max(1);
        let absorbed = steps[i].absorbed_by_roll;

        let effect = if absorbed {
            // E152 tie absorbed by a *N roll's spread schedule.
            slide_head = None;
            StepEffect::AbsorbedRoll
        } else if !gate && !trigger {
            // Rest.
            slide_head = None;
            StepEffect::Silent
        } else if trigger && gate {
            // Triggered cell — closes any prior open slide implicitly.
            slide_head = None;
            let roll = if repeat > 1 {
                Some(RollSpec { count: repeat, span: repeat_span })
            } else {
                None
            };
            let slide = cv1_end.map(|end| SlideOpen {
                close_cv1: end,
                closes_at_boundary: true,
            });
            if slide.is_some() {
                slide_head = Some(i);
            }
            StepEffect::StartNote { cv1, cv2, slide, roll }
        } else if gate && !trigger {
            // Tie. After a slide head it's a SlideFlow that extends the
            // open slide's close target; otherwise it's a plain Hold.
            if let Some(head_idx) = slide_head {
                if let Some(end) = cv1_end {
                    if let StepEffect::StartNote {
                        slide: Some(ref mut so),
                        ..
                    } = steps[head_idx].effect
                    {
                        so.close_cv1 = end;
                    }
                }
                StepEffect::SlideFlow
            } else {
                StepEffect::Hold
            }
        } else {
            // `trigger && !gate` — not produced by any current cell shape;
            // treat as Silent rather than panicking.
            slide_head = None;
            StepEffect::Silent
        };

        steps[i].effect = effect;
        i += 1;
    }
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

    #[test]
    fn pattern_bank_indexing() {
        let step = Step {
            cv1: 1.0,
            cv2: 0.5,
            trigger: true,
            gate: true,
            cv1_end: None,
            cv2_end: None,
            repeat: 1,
            repeat_span: 1,
            absorbed_by_roll: false,
            effect: StepEffect::Silent,
        };
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

    #[test]
    fn step_slide_fields() {
        let step = Step {
            cv1: 0.0,
            cv2: 0.0,
            trigger: true,
            gate: true,
            cv1_end: Some(1.0),
            cv2_end: Some(0.8),
            repeat: 3,
            repeat_span: 1,
            absorbed_by_roll: false,
            effect: StepEffect::Silent,
        };
        assert_eq!(step.cv1_end, Some(1.0));
        assert_eq!(step.cv2_end, Some(0.8));
        assert_eq!(step.repeat, 3);
    }

    fn trig(cv1: f32, repeat: u8) -> Step {
        Step { cv1, trigger: true, gate: true, repeat, ..Step::default() }
    }

    fn tie() -> Step {
        Step { gate: true, ..Step::default() }
    }

    fn rest() -> Step {
        Step::default()
    }

    fn note() -> Step {
        Step { cv1: 60.0, trigger: true, gate: true, ..Step::default() }
    }

    #[test]
    fn annotate_x3_alone_span_1() {
        let mut steps = vec![trig(1.0, 3)];
        annotate_repeat_spans(&mut steps);
        assert_eq!(steps[0].repeat_span, 1);
        assert!(!steps[0].absorbed_by_roll);
    }

    #[test]
    fn annotate_x3_tie_span_2() {
        let mut steps = vec![trig(1.0, 3), tie()];
        annotate_repeat_spans(&mut steps);
        assert_eq!(steps[0].repeat_span, 2);
        assert!(steps[1].absorbed_by_roll);
    }

    #[test]
    fn annotate_x5_tie_tie_span_3() {
        let mut steps = vec![trig(1.0, 5), tie(), tie()];
        annotate_repeat_spans(&mut steps);
        assert_eq!(steps[0].repeat_span, 3);
        assert!(steps[1].absorbed_by_roll);
        assert!(steps[2].absorbed_by_roll);
    }

    #[test]
    fn annotate_note_tie_unchanged() {
        let mut steps = vec![note(), tie()];
        annotate_repeat_spans(&mut steps);
        assert_eq!(steps[0].repeat_span, 1);
        assert_eq!(steps[1].repeat_span, 1);
        assert!(!steps[0].absorbed_by_roll);
        assert!(!steps[1].absorbed_by_roll);
    }

    #[test]
    fn annotate_x3_tie_note_span_2_note_untouched() {
        let mut steps = vec![trig(1.0, 3), tie(), note()];
        annotate_repeat_spans(&mut steps);
        assert_eq!(steps[0].repeat_span, 2);
        assert!(steps[1].absorbed_by_roll);
        assert_eq!(steps[2].repeat_span, 1);
        assert!(!steps[2].absorbed_by_roll);
    }

    #[test]
    fn annotate_chained_anchors() {
        // x*3 ~ ~ ~ note*2 ~  → anchor1 span=4, anchor2 span=2
        let mut steps = vec![trig(1.0, 3), tie(), tie(), tie(), trig(60.0, 2), tie()];
        annotate_repeat_spans(&mut steps);
        assert_eq!(steps[0].repeat_span, 4);
        assert!(steps[1].absorbed_by_roll);
        assert!(steps[2].absorbed_by_roll);
        assert!(steps[3].absorbed_by_roll);
        assert_eq!(steps[4].repeat_span, 2);
        assert!(steps[5].absorbed_by_roll);
    }

    #[test]
    fn annotate_rest_stops_span() {
        // x*3 ~ . ~  → anchor span=2, rest breaks the run, trailing tie not absorbed
        let mut steps = vec![trig(1.0, 3), tie(), rest(), tie()];
        annotate_repeat_spans(&mut steps);
        assert_eq!(steps[0].repeat_span, 2);
        assert!(steps[1].absorbed_by_roll);
        assert!(!steps[2].absorbed_by_roll);
        assert!(!steps[3].absorbed_by_roll);
    }

    #[test]
    fn annotate_truncates_at_row_end() {
        // x*3 ~ ~  → all three consumed, span=3
        let mut steps = vec![trig(1.0, 3), tie(), tie()];
        annotate_repeat_spans(&mut steps);
        assert_eq!(steps[0].repeat_span, 3);
        assert!(steps[1].absorbed_by_roll);
        assert!(steps[2].absorbed_by_roll);
    }

    #[test]
    fn annotate_is_idempotent() {
        let mut steps = vec![trig(1.0, 3), tie(), tie()];
        annotate_repeat_spans(&mut steps);
        let snapshot = steps.clone();
        annotate_repeat_spans(&mut steps);
        assert_eq!(steps, snapshot);
    }

    // ── E153 (ticket 0943): StepEffect resolution ────────────────────────

    fn slide_head(cv1: f32, end: f32) -> Step {
        Step {
            cv1,
            trigger: true,
            gate: true,
            cv1_end: Some(end),
            ..Step::default()
        }
    }

    fn slide_tail(cv1: f32, end: f32) -> Step {
        Step {
            cv1,
            trigger: false,
            gate: true,
            cv1_end: Some(end),
            ..Step::default()
        }
    }

    fn resolve(steps: &mut [Step]) {
        annotate_repeat_spans(steps);
        resolve_step_effects(steps);
    }

    #[test]
    fn resolve_value_cell_is_start_note() {
        let mut steps = vec![note()];
        resolve(&mut steps);
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
        let mut steps = vec![trig(60.0, 3)];
        resolve(&mut steps);
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
    fn resolve_value_x_n_with_span_absorbs_ties() {
        // x*3 ~ ~  → anchor span=3, two absorbed ties.
        let mut steps = vec![trig(60.0, 3), tie(), tie()];
        resolve(&mut steps);
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
        let mut steps = vec![note(), tie()];
        resolve(&mut steps);
        assert!(matches!(steps[0].effect, StepEffect::StartNote { .. }));
        assert!(matches!(steps[1].effect, StepEffect::Hold));
    }

    #[test]
    fn resolve_rest_is_silent() {
        let mut steps = vec![rest()];
        resolve(&mut steps);
        assert!(matches!(steps[0].effect, StepEffect::Silent));
    }

    #[test]
    fn resolve_slide_head_one_tick() {
        // value> value with cv1_end Some — head only, no tail.
        let mut steps = vec![slide_head(60.0, 72.0)];
        resolve(&mut steps);
        match &steps[0].effect {
            StepEffect::StartNote { slide: Some(so), cv1, .. } => {
                assert_eq!(*cv1, 60.0);
                assert_eq!(so.close_cv1, 72.0);
                assert!(so.closes_at_boundary);
            }
            other => panic!("expected StartNote+slide, got {other:?}"),
        }
    }

    #[test]
    fn resolve_slide_macro_two_tick() {
        // slide(2, A=60, C=72) lowers to head(60, end=66) + tail(66, end=72).
        // Resolved as StartNote+SlideOpen{close_cv1=72} followed by SlideFlow.
        let mut steps = vec![slide_head(60.0, 66.0), slide_tail(66.0, 72.0)];
        resolve(&mut steps);
        match &steps[0].effect {
            StepEffect::StartNote { slide: Some(so), cv1, .. } => {
                assert_eq!(*cv1, 60.0);
                assert_eq!(so.close_cv1, 72.0, "tail extends head's close_cv1 to final endpoint");
                assert!(so.closes_at_boundary);
            }
            other => panic!("expected StartNote+slide, got {other:?}"),
        }
        assert!(matches!(steps[1].effect, StepEffect::SlideFlow));
    }

    #[test]
    fn resolve_is_idempotent() {
        // Mixed channel: note, slide(2)-shaped pair, rest, value*3+ties.
        let mut steps = vec![
            note(),
            slide_head(60.0, 66.0),
            slide_tail(66.0, 72.0),
            rest(),
            trig(48.0, 3),
            tie(),
            tie(),
        ];
        resolve(&mut steps);
        let snapshot = steps.clone();
        // Re-run only the resolver; annotate_repeat_spans is also idempotent
        // but we exercise resolver-only here.
        resolve_step_effects(&mut steps);
        assert_eq!(steps, snapshot);
    }
}
