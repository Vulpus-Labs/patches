use super::*;
use patches_core::{Pattern, PatternBank, SongBank, TrackerStep};

const SR: f32 = 44100.0;

fn step(cv1: f32, cv2: f32, trigger: bool, gate: bool) -> TrackerStep {
    TrackerStep {
        cv1,
        cv2,
        trigger,
        gate,
        cv1_end: None,
        cv2_end: None,
        repeat: 1,
    }
}

fn data_single_pattern(rows: Vec<Vec<TrackerStep>>) -> TrackerData {
    let channels = rows.len();
    let steps = rows[0].len();
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

    let mut frame = ClockBusFrame::default();
    frame.tick_trigger = 1.0;
    frame.bank_index = 0.0;
    frame.tick_duration = 100.0 / SR;
    frame.step_index = 0.0;
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

    let mut frame = ClockBusFrame::default();
    frame.bank_index = 0.0;
    frame.tick_duration = 100.0 / SR;
    frame.step_index = 0.0;

    // Rising edge 1.
    frame.tick_trigger = 1.0;
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
        cv1_end: Some(cv1_end),
        cv2_end: Some(cv2_end),
        repeat: 1,
    }
}

fn repeat_step(cv1: f32, repeat: u8) -> TrackerStep {
    TrackerStep {
        cv1,
        cv2: 0.0,
        trigger: true,
        gate: true,
        cv1_end: None,
        cv2_end: None,
        repeat,
    }
}

fn rest() -> TrackerStep {
    TrackerStep {
        cv1: 0.0,
        cv2: 0.0,
        trigger: false,
        gate: false,
        cv1_end: None,
        cv2_end: None,
        repeat: 1,
    }
}

fn tie() -> TrackerStep {
    TrackerStep {
        cv1: 0.0,
        cv2: 0.0,
        trigger: false,
        gate: true,
        cv1_end: None,
        cv2_end: None,
        repeat: 1,
    }
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
    assert_eq!(core.repeat_count[0], 3);
    assert_eq!(core.repeat_index[0], 1, "first sub-trigger fires on apply_step");
    assert!((core.repeat_interval_samples[0] - 100.0).abs() < 1e-6);
}

#[test]
fn channel_count_mismatch_excess_pattern_channels_ignored() {
    // Pattern has 2 channels, core has 1 — excess pattern channels ignored.
    let data = TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 2,
                steps: 1,
                data: vec![
                    vec![step(1.0, 0.0, true, true)],
                    vec![step(2.0, 0.0, true, true)],
                ],
            }],
        },
        songs: SongBank { songs: vec![] },
    };
    let mut core = PatternPlayerCore::new(SR, 1);
    core.current_tick_duration_samples = 100.0;
    core.step_index[0] = 0;
    core.apply_step(0, 0, 0.0, &data);

    assert_eq!(core.cv1[0], 1.0, "channel 0 gets its data");
}

#[test]
fn channel_count_mismatch_surplus_core_channels_silent() {
    // Pattern has 1 channel, core has 2 — surplus core channels silent.
    let data = TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 1,
                steps: 1,
                data: vec![vec![step(1.0, 0.0, true, true)]],
            }],
        },
        songs: SongBank { songs: vec![] },
    };
    let mut core = PatternPlayerCore::new(SR, 2);
    core.current_tick_duration_samples = 100.0;
    core.step_index[0] = 0;
    core.step_index[1] = 0;
    core.apply_step(0, 0, 0.0, &data);
    core.apply_step(1, 0, 0.0, &data);

    assert_eq!(core.cv1[0], 1.0, "channel 0 has data");
    assert!(!core.gate[1], "surplus channel is silent");
}

#[test]
fn stop_sentinel_clears_all() {
    let mut core = PatternPlayerCore::new(SR, 2);
    core.cv1[0] = 5.0;
    core.cv1[1] = 3.0;
    core.gate[0] = true;
    core.gate[1] = true;

    let mut frame = ClockBusFrame::default();
    frame.tick_trigger = 1.0;
    frame.bank_index = -1.0;
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
