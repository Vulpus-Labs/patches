use super::*;
use patches_core::{
    resolve_step_effects, Pattern, PatternBank, SongBank, StepKind, TrackerStep,
};

const SR: f32 = 44100.0;

fn step(cv1: f32, cv2: f32, trigger: bool, gate: bool) -> TrackerStep {
    let kind = if trigger {
        StepKind::Note { repeat: 1 }
    } else if gate {
        StepKind::Tie
    } else {
        StepKind::Rest
    };
    TrackerStep { cv1, cv2, trigger, gate, kind, ..TrackerStep::default() }
}

/// Build a single-pattern [`TrackerData`] with the E153 row-build pass
/// applied per channel. Mirrors what `build_tracker_data` does in the
/// interpreter so apply_step (which dispatches on `step.effect`) sees
/// resolved effects rather than the default `Silent`.
fn data_single_pattern(mut rows: Vec<Vec<TrackerStep>>) -> TrackerData {
    let channels = rows.len();
    let steps = rows[0].len();
    for ch in rows.iter_mut() {
        let errs = resolve_step_effects(ch);
        assert!(errs.is_empty(), "row-build errors in test pattern: {errs:?}");
    }
    TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels,
                steps,
                data: rows,
            }],
        },
        songs: SongBank { songs: vec![] },
    }
}

#[test]
fn apply_step_note_event_sets_cv_gate_trigger() {
    let data = data_single_pattern(vec![vec![step(1.25, 0.5, true, true)]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    core.current_tick_duration_samples = 100.0;
    core.apply_step(0, 0, 0.0, &data);

    assert_eq!(core.cv1[0], 1.25);
    assert_eq!(core.cv2[0], 0.5);
    assert!(core.gate[0]);
    assert!(core.trigger_pending[0]);
}

#[test]
fn tick_without_edge_holds_previous_values() {
    // Advance one step on a rising edge, then tick with no edge — outputs
    // should carry over (minus the single-sample trigger pulse).
    let data = data_single_pattern(vec![vec![step(2.0, 0.25, true, true)]]);
    let mut core = PatternPlayerCore::new(SR, 1);

    let mut frame = ClockBusFrame {
        tick_trigger: 1.0,
        bank_index: 0.0,
        tick_duration: 100.0 / SR,
        step_index: 0.0,
        ..Default::default()
    };
    core.tick(&frame, &data);
    assert!(core.trigger_pending[0]);
    let cv1_after_edge = core.cv1[0];
    let cv2_after_edge = core.cv2[0];

    // No rising edge next sample.
    frame.tick_trigger = 1.0; // stays high — not a *rising* edge
    core.tick(&frame, &data);
    assert!(!core.trigger_pending[0], "trigger must clear on non-edge sample");
    assert_eq!(core.cv1[0], cv1_after_edge);
    assert_eq!(core.cv2[0], cv2_after_edge);
    assert!(core.gate[0]);
}

#[test]
fn trigger_edge_detect_fires_once_per_rising_edge() {
    let data = data_single_pattern(vec![vec![step(1.0, 0.0, true, true)]]);
    let mut core = PatternPlayerCore::new(SR, 1);

    let mut frame = ClockBusFrame {
        bank_index: 0.0,
        tick_duration: 100.0 / SR,
        step_index: 0.0,
        tick_trigger: 1.0,
        ..Default::default()
    };
    // Rising edge 1.
    core.tick(&frame, &data);
    assert!(core.trigger_pending[0]);

    // Trigger held high — no new edge.
    core.tick(&frame, &data);
    assert!(!core.trigger_pending[0]);

    // Trigger drops, no edge.
    frame.tick_trigger = 0.0;
    core.tick(&frame, &data);
    assert!(!core.trigger_pending[0]);

    // Rising edge 2.
    frame.tick_trigger = 1.0;
    core.tick(&frame, &data);
    assert!(core.trigger_pending[0]);
}

fn slide_step(cv1: f32, cv1_end: f32, cv2: f32, cv2_end: f32) -> TrackerStep {
    TrackerStep {
        cv1,
        cv2,
        trigger: true,
        gate: true,
        kind: StepKind::SlideSugar { cv1_end, cv2_end: Some(cv2_end) },
        ..TrackerStep::default()
    }
}

fn repeat_step(cv1: f32, repeat: u8) -> TrackerStep {
    TrackerStep {
        cv1,
        trigger: true,
        gate: true,
        kind: StepKind::Note { repeat },
        ..TrackerStep::default()
    }
}

/// E152 spread roll: `*N` anchor. Span is derived by the row-build
/// pass from the absorbed tie cells that follow this anchor; the test
/// helper only needs to set the anchor's kind.
fn roll_anchor(cv1: f32, repeat: u8, _span: u8) -> TrackerStep {
    TrackerStep {
        cv1,
        trigger: true,
        gate: true,
        kind: StepKind::Note { repeat },
        ..TrackerStep::default()
    }
}

/// E152 absorbed tie cell. Absorption is decided by the row-build pass
/// based on the preceding `*N` anchor — the helper just emits a `Tie`
/// cell and lets the resolver classify it.
fn absorbed_tie() -> TrackerStep {
    TrackerStep { gate: true, kind: StepKind::Tie, ..TrackerStep::default() }
}

fn rest() -> TrackerStep {
    TrackerStep::default()
}

fn tie() -> TrackerStep {
    TrackerStep { gate: true, kind: StepKind::Tie, ..TrackerStep::default() }
}

#[test]
fn basic_step_playback_across_three_steps() {
    let data = data_single_pattern(vec![vec![
        step(1.0, 0.5, true, true),
        step(2.0, 0.8, true, true),
        rest(),
    ]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    core.current_tick_duration_samples = 0.125 * SR;

    core.step_index[0] = 0;
    core.apply_step(0, 0, 0.0, &data);
    assert_eq!(core.cv1[0], 1.0);
    assert_eq!(core.cv2[0], 0.5);
    assert!(core.gate[0]);
    assert!(core.trigger_pending[0]);

    core.step_index[0] = 1;
    core.apply_step(0, 0, 0.0, &data);
    assert_eq!(core.cv1[0], 2.0);
    assert_eq!(core.cv2[0], 0.8);
    assert!(core.gate[0]);
    assert!(core.trigger_pending[0]);

    core.step_index[0] = 2;
    core.apply_step(0, 0, 0.0, &data);
    assert!(!core.gate[0], "rest drops gate");
    assert!(!core.trigger_pending[0], "rest emits no trigger");
}

#[test]
fn tie_holds_gate_and_carries_cv() {
    // Step 0: note. Step 1: tie (trigger=false, gate=true, no slide
    // targets) — gate stays high, trigger does not fire, cv carries.
    let data = data_single_pattern(vec![vec![
        step(3.0, 0.0, true, true),
        tie(),
    ]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    core.current_tick_duration_samples = 0.125 * SR;

    core.step_index[0] = 0;
    core.apply_step(0, 0, 0.0, &data);
    assert_eq!(core.cv1[0], 3.0);
    assert!(core.gate[0]);
    assert!(core.trigger_pending[0]);

    core.step_index[0] = 1;
    core.apply_step(0, 0, 0.0, &data);
    assert!(core.gate[0], "tie keeps gate high");
    assert!(!core.trigger_pending[0], "tie emits no trigger");
    assert_eq!(core.cv1[0], 3.0, "cv carries over on tie");
}

#[test]
fn slide_interpolation_sets_ramp_state() {
    let data = data_single_pattern(vec![vec![slide_step(0.0, 1.0, 0.0, 0.5)]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    core.current_tick_duration_samples = 100.0;

    core.step_index[0] = 0;
    core.apply_step(0, 0, 0.0, &data);

    assert!(core.slide_active[0]);
    assert_eq!(core.slide_cv1_start[0], 0.0);
    assert_eq!(core.slide_cv1_end[0], 1.0);
    assert_eq!(core.slide_cv2_start[0], 0.0);
    assert_eq!(core.slide_cv2_end[0], 0.5);
    assert_eq!(core.slide_samples_total[0], 100.0);

    // Halfway through the slide.
    core.slide_samples_elapsed[0] = 50.0;
    let t = 50.0_f32 / 100.0;
    let expected_cv1 = core.slide_cv1_start[0]
        + t * (core.slide_cv1_end[0] - core.slide_cv1_start[0]);
    assert!((expected_cv1 - 0.5).abs() < 1e-6);
}

#[test]
fn repeat_subdivision_schedules_sub_triggers() {
    let data = data_single_pattern(vec![vec![repeat_step(1.0, 3)]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    core.current_tick_duration_samples = 300.0;

    core.step_index[0] = 0;
    core.apply_step(0, 0, 0.0, &data);

    assert!(core.repeat_active[0]);
    assert_eq!(core.sub_events[0].len(), 3);
    assert_eq!(core.sub_event_head[0], 1, "first sub-trigger fires on apply_step");
    assert_eq!(core.sub_events[0][0].tick_idx_in_span, 0);
    assert!(core.sub_events[0][0].fraction.abs() < 1e-6);
    assert_eq!(core.sub_events[0][1].tick_idx_in_span, 0);
    assert!((core.sub_events[0][1].fraction - 1.0 / 3.0).abs() < 1e-6);
    assert_eq!(core.sub_events[0][2].tick_idx_in_span, 0);
    assert!((core.sub_events[0][2].fraction - 2.0 / 3.0).abs() < 1e-6);
}

#[test]
fn channel_count_mismatch_excess_pattern_channels_ignored() {
    // Pattern has 2 channels, core has 1 — excess pattern channels ignored.
    let data = data_single_pattern(vec![
        vec![step(1.0, 0.0, true, true)],
        vec![step(2.0, 0.0, true, true)],
    ]);
    let mut core = PatternPlayerCore::new(SR, 1);
    core.current_tick_duration_samples = 100.0;
    core.step_index[0] = 0;
    core.apply_step(0, 0, 0.0, &data);

    assert_eq!(core.cv1[0], 1.0, "channel 0 gets its data");
}

#[test]
fn channel_count_mismatch_surplus_core_channels_silent() {
    // Pattern has 1 channel, core has 2 — surplus core channels silent.
    let data = data_single_pattern(vec![vec![step(1.0, 0.0, true, true)]]);
    let mut core = PatternPlayerCore::new(SR, 2);
    core.current_tick_duration_samples = 100.0;
    core.step_index[0] = 0;
    core.step_index[1] = 0;
    core.apply_step(0, 0, 0.0, &data);
    core.apply_step(1, 0, 0.0, &data);

    assert_eq!(core.cv1[0], 1.0, "channel 0 has data");
    assert!(!core.gate[1], "surplus channel is silent");
}

// ── E152 tie-spread roll (ticket 0940) ────────────────────────────────

/// Drive `tick()` sample-by-sample across `tick_count` ticks, emitting a
/// rising-edge frame on the first sample of each tick. Returns the
/// channel-0 sample indices at which `trigger_pending[0]` was true after
/// each `tick()` call.
fn run_roll(
    core: &mut PatternPlayerCore,
    data: &TrackerData,
    tick_samples: usize,
    tick_count: usize,
) -> Vec<usize> {
    let mut trigger_samples = Vec::new();
    let mut sample = 0usize;
    for tick_i in 0..tick_count {
        for s in 0..tick_samples {
            let frame = ClockBusFrame {
                tick_trigger: if s == 0 { 1.0 } else { 0.0 },
                bank_index: 0.0,
                tick_duration: tick_samples as f32 / SR,
                step_index: tick_i as f32,
                step_fraction: s as f32 / tick_samples as f32,
                pattern_reset: 0.0,
            };
            core.tick(&frame, data);
            if core.trigger_pending[0] {
                trigger_samples.push(sample);
            }
            sample += 1;
        }
    }
    trigger_samples
}

#[test]
fn spread_span_1_is_bit_identical_to_pre_e152() {
    // `x*3` alone — span=1 must reproduce the pre-E152 single-tick
    // formula `interval = tick / N`. Triggers at samples 0, 100, 200.
    let data = data_single_pattern(vec![vec![roll_anchor(1.0, 3, 1)]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let triggers = run_roll(&mut core, &data, 300, 1);
    assert_eq!(triggers, vec![0, 100, 200]);
    assert!(!core.repeat_active[0], "roll finishes within the tick");
}

#[test]
fn spread_x3_tie_three_triggers_across_two_ticks() {
    // `x*3 ~` — span=2, N=3. Interval = 2 * 300 / 3 = 200.
    // Triggers at samples 0, 200, 400.
    let data = data_single_pattern(vec![vec![roll_anchor(1.0, 3, 2), absorbed_tie()]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let triggers = run_roll(&mut core, &data, 300, 2);
    assert_eq!(triggers, vec![0, 200, 400]);
}

#[test]
fn spread_x5_tie_tie_five_triggers_across_three_ticks() {
    // `x*5 ~ ~` — span=3, N=5. Interval = 3 * 300 / 5 = 180.
    // Triggers at samples 0, 180, 360, 540, 720.
    let data = data_single_pattern(vec![vec![
        roll_anchor(1.0, 5, 3),
        absorbed_tie(),
        absorbed_tie(),
    ]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let triggers = run_roll(&mut core, &data, 300, 3);
    assert_eq!(triggers, vec![0, 180, 360, 540, 720]);
    // Last trigger sits inside the span: 720 < 3 * 300 = 900.
    assert!(*triggers.last().unwrap() < 3 * 300);
}

/// Record per-sample `(gate, trigger_pending)` state for `tick_count`
/// ticks of `tick_samples` each, with a rising-edge frame on the first
/// sample of each tick. Useful for asserting both schedule-level and
/// gate-articulation behaviour off one drive.
fn record_roll(
    core: &mut PatternPlayerCore,
    data: &TrackerData,
    tick_samples: usize,
    tick_count: usize,
) -> Vec<(bool, bool)> {
    let mut out = Vec::with_capacity(tick_samples * tick_count);
    for tick_i in 0..tick_count {
        for s in 0..tick_samples {
            let frame = ClockBusFrame {
                tick_trigger: if s == 0 { 1.0 } else { 0.0 },
                bank_index: 0.0,
                tick_duration: tick_samples as f32 / SR,
                step_index: tick_i as f32,
                step_fraction: s as f32 / tick_samples as f32,
                pattern_reset: 0.0,
            };
            core.tick(&frame, data);
            out.push((core.gate[0], core.trigger_pending[0]));
        }
    }
    out
}

#[test]
fn spread_absorbed_tie_does_not_fire_independent_trigger() {
    // `x*3 ~` schedule fires at samples 0, 200, 400. The tie tick rises
    // at sample 300 — must not inject a trigger there.
    let data = data_single_pattern(vec![vec![roll_anchor(1.0, 3, 2), absorbed_tie()]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let trace = record_roll(&mut core, &data, 300, 2);
    let trigger_samples: Vec<usize> = trace
        .iter()
        .enumerate()
        .filter_map(|(i, (_, trig))| if *trig { Some(i) } else { None })
        .collect();
    assert_eq!(trigger_samples, vec![0, 200, 400]);
    assert!(!trace[300].1, "absorbed-tie tick rise emits no trigger");
}

#[test]
fn spread_gate_articulation_scales_with_longer_interval() {
    // `x*3 ~`: interval = 200 → 0.8-window = 160. After the first
    // trigger at sample 0, gate must still be high at sample 159 and
    // drop by sample 160; the next sub-trigger at sample 200 re-raises
    // it.
    let data = data_single_pattern(vec![vec![roll_anchor(1.0, 3, 2), absorbed_tie()]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let trace = record_roll(&mut core, &data, 300, 2);
    assert!(trace[159].0, "gate high just before the 0.8 * interval window expires");
    assert!(!trace[160].0, "gate drops at sample 160 = 0.8 * interval");
    assert!(trace[200].0, "gate re-rises with the second sub-trigger");
}

#[test]
fn spread_mid_step_entry_uses_span_interval() {
    // Enter the anchor mid-step (step_fraction = 0.25) with a `x*3 ~`
    // (span=2) anchor. Schedule = [(0,0), (0,2/3), (1,1/3)]; anchor
    // consumed inline so head=1. Gate-off countdown to next sub-event
    // = 0.8 * ((2/3 * 300) - 75) = 0.8 * 125 = 100.
    let data = data_single_pattern(vec![vec![roll_anchor(1.0, 3, 2), absorbed_tie()]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    core.current_tick_duration_samples = 300.0;
    core.step_index[0] = 0;
    core.apply_step(0, 0, 0.25, &data);
    assert!(core.repeat_active[0]);
    assert_eq!(core.sub_events[0].len(), 3);
    assert_eq!(core.sub_event_head[0], 1);
    assert_eq!(core.sub_events[0][1].tick_idx_in_span, 0);
    assert!((core.sub_events[0][1].fraction - 2.0 / 3.0).abs() < 1e-6);
    assert_eq!(core.sub_events[0][2].tick_idx_in_span, 1);
    assert!((core.sub_events[0][2].fraction - 1.0 / 3.0).abs() < 1e-6);
    assert!((core.repeat_gate_off_countdown[0] - 100.0).abs() < 1e-3);
}

#[test]
fn spread_non_absorbed_cell_completes_in_flight_roll_at_its_boundary() {
    // Mid-roll live-edit semantics: a cell that the row-build *thought*
    // was absorbed but the runtime data was hot-swapped to mark it as
    // non-absorbed (or a different non-absorbed cell entirely) must
    // re-apply normally on its own tick rise — the in-flight schedule
    // dies cleanly at that boundary rather than overrunning.
    //
    // Simulate by starting with `x*3 ~`, then before the second tick
    // rises, swap the tracker so step_index=1 is a fresh note (a
    // non-absorbed cell). The roll should NOT survive into tick 1.
    let before = data_single_pattern(vec![vec![roll_anchor(1.0, 3, 2), absorbed_tie()]]);
    let after = data_single_pattern(vec![vec![
        roll_anchor(1.0, 3, 2),
        step(7.0, 0.0, true, true),
    ]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let tick_samples = 300;

    // Tick 0 — anchor rises. Use the original tracker.
    let frame_tick0_rise = ClockBusFrame {
        tick_trigger: 1.0,
        bank_index: 0.0,
        tick_duration: tick_samples as f32 / SR,
        step_index: 0.0,
        step_fraction: 0.0,
        ..Default::default()
    };
    core.tick(&frame_tick0_rise, &before);
    assert!(core.repeat_active[0]);

    // Tick 1 rises against the EDITED tracker: cell 1 is now a real
    // note (cv1=7, trigger=true). apply_step must process it normally.
    let frame_tick1_rise = ClockBusFrame {
        tick_trigger: 1.0,
        bank_index: 0.0,
        tick_duration: tick_samples as f32 / SR,
        step_index: 1.0,
        step_fraction: 0.0,
        ..Default::default()
    };
    // First a non-edge sample to clear prev_tick_trigger.
    let mut frame_no_edge = frame_tick0_rise;
    frame_no_edge.tick_trigger = 0.0;
    core.tick(&frame_no_edge, &before);
    core.tick(&frame_tick1_rise, &after);
    assert_eq!(core.cv1[0], 7.0, "edited cell takes over on its tick");
    assert!(core.trigger_pending[0], "edited cell fires its own trigger");
    assert!(
        !core.repeat_active[0],
        "the prior roll is replaced — non-absorbed cell owns the channel"
    );
}

// ── E153 ticket 0945: per-tick swung sub-event schedule ─────────────

/// Like `run_roll` but with per-tick sample lengths. Used to drive the
/// player across a swung tick boundary (alternating long/short ticks).
fn run_roll_swung(
    core: &mut PatternPlayerCore,
    data: &TrackerData,
    tick_samples_per_tick: &[usize],
) -> Vec<usize> {
    let mut trigger_samples = Vec::new();
    let mut sample = 0usize;
    for (tick_i, &tick_samples) in tick_samples_per_tick.iter().enumerate() {
        for s in 0..tick_samples {
            let frame = ClockBusFrame {
                tick_trigger: if s == 0 { 1.0 } else { 0.0 },
                bank_index: 0.0,
                tick_duration: tick_samples as f32 / SR,
                step_index: tick_i as f32,
                step_fraction: s as f32 / tick_samples as f32,
                pattern_reset: 0.0,
            };
            core.tick(&frame, data);
            if core.trigger_pending[0] {
                trigger_samples.push(sample);
            }
            sample += 1;
        }
    }
    trigger_samples
}

#[test]
fn non_swung_x3_tilde_bit_identical_to_e152() {
    // `x*3 ~` at swing=0.5 (uniform 300-sample ticks) — schedule
    // collapses to the E152 uniform-interval result.
    let data = data_single_pattern(vec![vec![roll_anchor(1.0, 3, 2), absorbed_tie()]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let triggers = run_roll_swung(&mut core, &data, &[300, 300]);
    assert_eq!(triggers, vec![0, 200, 400]);
}

#[test]
fn non_swung_x3_alone_bit_identical_to_pre_e152() {
    // `x*3` alone at swing=0.5 — pre-E152 single-tick `interval = T/3`
    // result. Triggers land at samples 0, T/3, 2T/3.
    let data = data_single_pattern(vec![vec![roll_anchor(1.0, 3, 1)]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let triggers = run_roll_swung(&mut core, &data, &[300]);
    assert_eq!(triggers, vec![0, 100, 200]);
}

#[test]
fn swung_x3_tilde_uses_per_tick_durations() {
    // `x*3 ~` at swing=0.66 — anchor lands on the lengthened even tick
    // (396 samples), absorbed tie on the shortened odd tick (204).
    // Hand-computed sub-trigger placement:
    //   t_0 = (0, 0)       → sample 0
    //   t_1 = (0, 2/3)     → 2/3 * 396 = 264 in tick 0 → abs 264
    //   t_2 = (1, 1/3)     → 1/3 * 204 = 68  in tick 1 → abs 396 + 68 = 464
    let data = data_single_pattern(vec![vec![roll_anchor(1.0, 3, 2), absorbed_tie()]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let triggers = run_roll_swung(&mut core, &data, &[396, 204]);
    assert_eq!(triggers, vec![0, 264, 464]);
}

#[test]
fn swung_x5_tilde_tilde_per_tick_durations() {
    // `x*5 ~ ~` at swing=0.66 — three ticks, durations 396, 204, 396.
    // t_k = k/5 * 3 → split as (tick_idx, fraction):
    //   t_0 = (0, 0.0)       → sample 0
    //   t_1 = (0, 0.6)       → 0.6 * 396 = 237.6 → ceil(237.6) = 238 (sample 238)
    //   t_2 = (1, 0.2)       → 0.2 * 204 = 40.8 → 41 in tick 1 → abs 396 + 41 = 437
    //   t_3 = (1, 0.8)       → 0.8 * 204 = 163.2 → 164 in tick 1 → abs 396 + 164 = 560
    //   t_4 = (2, 0.4)       → 0.4 * 396 = 158.4 → 159 in tick 2 → abs 600 + 159 = 759
    let data = data_single_pattern(vec![vec![
        roll_anchor(1.0, 5, 3),
        absorbed_tie(),
        absorbed_tie(),
    ]]);
    let mut core = PatternPlayerCore::new(SR, 1);
    let triggers = run_roll_swung(&mut core, &data, &[396, 204, 396]);
    assert_eq!(triggers, vec![0, 238, 437, 560, 759]);
}

#[test]
fn stop_sentinel_clears_all() {
    let mut core = PatternPlayerCore::new(SR, 2);
    core.cv1[0] = 5.0;
    core.cv1[1] = 3.0;
    core.gate[0] = true;
    core.gate[1] = true;

    let frame = ClockBusFrame {
        tick_trigger: 1.0,
        bank_index: -1.0,
        ..Default::default()
    };
    let data = TrackerData {
        patterns: PatternBank { patterns: vec![] },
        songs: SongBank { songs: vec![] },
    };
    core.tick(&frame, &data);

    assert_eq!(core.cv1[0], 0.0);
    assert_eq!(core.cv1[1], 0.0);
    assert!(!core.gate[0]);
    assert!(!core.gate[1]);
    assert!(core.stopped);
}
