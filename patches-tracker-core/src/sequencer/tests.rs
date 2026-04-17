use super::*;
use patches_core::{Pattern, PatternBank, Song, SongBank, TrackerStep};

const SR: f32 = 44100.0;

fn simple_step(cv1: f32) -> TrackerStep {
    TrackerStep {
        cv1,
        cv2: 0.0,
        trigger: true,
        gate: true,
        cv1_end: None,
        cv2_end: None,
        repeat: 1,
    }
}

fn single_pattern(steps: usize) -> Pattern {
    Pattern {
        channels: 1,
        steps,
        data: vec![(0..steps).map(|i| simple_step(i as f32)).collect()],
    }
}

fn tracker_one_song(order: Vec<Vec<Option<usize>>>, loop_point: usize, patterns: Vec<Pattern>) -> TrackerData {
    TrackerData {
        patterns: PatternBank { patterns },
        songs: SongBank {
            songs: vec![Song {
                channels: 1,
                order,
                loop_point,
            }],
        },
    }
}

fn start_core(bpm: f32, rows_per_beat: i64, swing: f32) -> SequencerCore {
    let mut core = SequencerCore::new(SR, 1);
    core.set_tempo(bpm, rows_per_beat, swing);
    core.set_song(Some(0));
    core.set_loop(true);
    core.start_playback();
    core
}

#[test]
fn deterministic_step_advance_constant_tempo() {
    // 120 BPM, 4 rows/beat → tick every 0.125 s = 5512.5 samples at 44.1 kHz.
    let tracker = tracker_one_song(
        vec![vec![Some(0)]],
        0,
        vec![single_pattern(4)],
    );
    let mut core = start_core(120.0, 4, 0.5);

    let edges = TransportEdges::default();
    // First sample after start_playback: first_tick fires.
    let r0 = core.tick_free(&edges, &tracker);
    assert!(r0.tick_fired, "first tick should fire on start");
    assert!(r0.reset_fired, "first tick should set reset");

    // Count ticks over two patterns worth of samples.
    let expected_samples_per_tick = (SR * 60.0 / (120.0 * 4.0)) as usize; // 5512
    let total_ticks_expected = 8;
    let total_samples = expected_samples_per_tick * total_ticks_expected + expected_samples_per_tick / 2;

    let mut tick_count = 1; // counted r0
    for _ in 0..total_samples {
        let r = core.tick_free(&edges, &tracker);
        if r.tick_fired {
            tick_count += 1;
        }
    }
    // Allow ±1 drift due to fractional samples-per-tick.
    assert!(
        (tick_count as i64 - total_ticks_expected as i64 - 1).abs() <= 1,
        "expected ~{} ticks, counted {tick_count}",
        total_ticks_expected + 1
    );
}

#[test]
fn swing_alternates_tick_durations() {
    let mut core = SequencerCore::new(SR, 1);
    core.set_tempo(120.0, 4, 0.6);
    let even_dur = core.tick_duration_seconds(0);
    let odd_dur = core.tick_duration_seconds(1);
    let base = 60.0_f32 / (120.0 * 4.0);
    assert!((even_dur - 2.0 * base * 0.6).abs() < 1e-6);
    assert!((odd_dur - 2.0 * base * 0.4).abs() < 1e-6);
    // swing=0.5 falls back to the base duration.
    core.set_tempo(120.0, 4, 0.5);
    assert!((core.tick_duration_seconds(0) - base).abs() < 1e-6);
    assert!((core.tick_duration_seconds(1) - base).abs() < 1e-6);
}

#[test]
fn loop_point_wraps_back_when_loop_enabled() {
    // Song with 3 rows, loop_point = 1. Advancing past the end should land
    // at row 1, not row 0.
    let tracker = tracker_one_song(
        vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
        1,
        vec![single_pattern(2)],
    );
    let mut core = start_core(120.0, 4, 0.5);
    core.do_loop = true;

    // Start from first_tick state. Fill indices.
    core.fill_bank_indices(&tracker);

    // Walk through steps by calling advance_step directly.
    // Pattern has 2 steps per row; song has 3 rows → total 6 steps before wrap.
    for _ in 0..5 {
        assert!(core.advance_step(&tracker));
    }
    // After 5 advances we've crossed from row 0 → 1 → 2; the next advance
    // should wrap back to loop_point (1).
    assert!(core.advance_step(&tracker));
    assert_eq!(core.song_row, 1, "advance past end with loop should land at loop_point");
    assert!(!core.song_ended);
}

// ── tick_duration_seconds / samples math ───────────────────────────────

#[test]
fn tick_duration_math_at_120_bpm_4_rows() {
    // 120 BPM, 4 rows/beat → tick every 60/(120*4) = 0.125s.
    // At SR=44100 that's 5512.5 samples.
    let mut core = SequencerCore::new(SR, 1);
    core.set_tempo(120.0, 4, 0.5);
    let dur = core.tick_duration_seconds(0);
    assert!((dur - 0.125).abs() < 1e-6);
    let samples = core.tick_duration_samples(0);
    assert!((samples - 5512.5).abs() < 1.0);
}

// ── tick_free transport edges ──────────────────────────────────────────

/// Walk the free-run transport through start → pause → resume → stop,
/// verifying `state` transitions on each rising edge. Replaces the old
/// `transport_state_machine` module test which poked `core.state` directly;
/// this version exercises the actual edge-detection logic in `tick_free`.
#[test]
fn transport_edge_state_transitions() {
    let tracker = tracker_one_song(
        vec![vec![Some(0)]],
        0,
        vec![single_pattern(4)],
    );
    let mut core = SequencerCore::new(SR, 1);
    core.set_tempo(120.0, 4, 0.5);
    core.set_song(Some(0));
    core.set_loop(true);
    // No autostart — stays Stopped until a start edge.
    assert_eq!(core.state, TransportState::Stopped);

    // Rising start edge → Playing.
    let edges = TransportEdges { start: 1.0, ..Default::default() };
    core.tick_free(&edges, &tracker);
    assert_eq!(core.state, TransportState::Playing);

    // Start held high → no new edge, stays Playing.
    core.tick_free(&edges, &tracker);
    assert_eq!(core.state, TransportState::Playing);

    // Rising pause edge → Paused.
    let edges_pause = TransportEdges { start: 1.0, pause: 1.0, ..Default::default() };
    core.tick_free(&edges_pause, &tracker);
    assert_eq!(core.state, TransportState::Paused);

    // Rising resume edge (pause stays high so no new pause edge) → Playing.
    let edges_resume = TransportEdges { start: 1.0, pause: 1.0, resume: 1.0, ..Default::default() };
    core.tick_free(&edges_resume, &tracker);
    assert_eq!(core.state, TransportState::Playing);

    // Rising stop edge → Stopped (and resets position).
    let edges_stop = TransportEdges { start: 1.0, pause: 1.0, resume: 1.0, stop: 1.0 };
    core.tick_free(&edges_stop, &tracker);
    assert_eq!(core.state, TransportState::Stopped);
    assert_eq!(core.song_row, 0);
    assert_eq!(core.pattern_step, 0);
}

// ── tick_host behaviour ────────────────────────────────────────────────

fn host_env(playing: f32, tempo: f32, beat: f64, tsig_num: f64, tsig_denom: f64) -> HostTransport {
    HostTransport { playing, tempo, beat, tsig_num, tsig_denom }
}

fn host_core(tracker_loop: bool) -> SequencerCore {
    let mut core = SequencerCore::new(SR, 1);
    core.set_song(Some(0));
    core.set_loop(tracker_loop);
    // No autostart in host mode — state changes on playing edge.
    core
}

#[test]
fn host_sync_first_tick_fires_on_playing_edge() {
    let tracker = tracker_one_song(
        vec![vec![Some(0)]],
        0,
        vec![single_pattern(4)],
    );
    let mut core = host_core(true);

    // Playing = 0 → no tick.
    let r = core.tick_host(&host_env(0.0, 120.0, 0.0, 4.0, 4.0), &tracker);
    assert!(!r.tick_fired);

    // Playing = 1 (rising edge) → tick fires, reset fires.
    let r = core.tick_host(&host_env(1.0, 120.0, 0.0, 4.0, 4.0), &tracker);
    assert!(r.tick_fired, "tick should fire on playing edge");
    assert!(r.reset_fired, "reset should fire on first tick");
    assert_eq!(core.state, TransportState::Playing);
}

#[test]
fn host_sync_freezes_on_stop_edge() {
    let tracker = tracker_one_song(
        vec![vec![Some(0)]],
        0,
        vec![single_pattern(4)],
    );
    let mut core = host_core(true);

    // Start, advance to beat 1.0 (bar_fraction 0.25 in 4/4, step 1 of 4).
    core.tick_host(&host_env(1.0, 120.0, 0.0, 4.0, 4.0), &tracker);
    core.tick_host(&host_env(1.0, 120.0, 1.0, 4.0, 4.0), &tracker);
    assert_eq!(core.pattern_step, 1);

    // Stop (1 → 0). Position freezes — state goes Paused, not reset.
    core.tick_host(&host_env(0.0, 120.0, 1.0, 4.0, 4.0), &tracker);
    assert_eq!(core.state, TransportState::Paused);
    assert_eq!(core.pattern_step, 1, "position preserved after host stop");
}

#[test]
fn host_sync_mid_song_start_positions_at_target_row() {
    // 4 rows of 4-step patterns. Starting at beat 8.0 in 4/4 → bar 2, step 0.
    let tracker = tracker_one_song(
        vec![vec![Some(0)], vec![Some(1)], vec![Some(2)], vec![Some(3)]],
        0,
        vec![
            single_pattern(4),
            single_pattern(4),
            single_pattern(4),
            single_pattern(4),
        ],
    );
    let mut core = host_core(true);

    let r = core.tick_host(&host_env(1.0, 120.0, 8.0, 4.0, 4.0), &tracker);
    assert!(r.tick_fired);
    assert!(r.reset_fired);
    assert_eq!(core.song_row, 2);
    assert_eq!(core.pattern_step, 0);
    assert_eq!(core.bank_indices[0], 2.0, "bank index should match row 2's pattern");
}

#[test]
fn host_sync_mid_bar_start_positions_at_step_and_fraction() {
    // 8-step pattern. Beat 9.7 in 4/4: bar 2, bar_fraction 0.425,
    // step = floor(0.425 * 8) = 3; step_fraction = 0.425*8 − 3 = 0.4.
    let tracker = tracker_one_song(
        vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
        0,
        vec![single_pattern(8)],
    );
    let mut core = host_core(true);

    core.tick_host(&host_env(1.0, 120.0, 9.7, 4.0, 4.0), &tracker);
    assert_eq!(core.song_row, 2);
    assert_eq!(core.pattern_step, 3);
    assert!(core.step_fraction > 0.3 && core.step_fraction < 0.5, "step_fraction {}", core.step_fraction);
}

#[test]
fn host_sync_three_four_time() {
    // 3/4: beats_per_bar = 3. Beat 7.5 → bar 2 (7.5/3 = 2.5),
    // bar_fraction = 0.5, step = floor(0.5 * 4) = 2.
    let tracker = tracker_one_song(
        vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
        0,
        vec![single_pattern(4)],
    );
    let mut core = host_core(true);

    core.tick_host(&host_env(1.0, 120.0, 7.5, 3.0, 4.0), &tracker);
    assert_eq!(core.song_row, 2);
    assert_eq!(core.pattern_step, 2);
}

#[test]
fn host_sync_loop_wrapping_past_song_end() {
    // 3 rows, loop_point = 1. Bar 5 in 4/4 (beat 20.0):
    // past-end = 5 - 3 = 2, loop_len = 2 → row = loop_point + 2 % 2 = 1.
    let tracker = tracker_one_song(
        vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
        1,
        vec![single_pattern(4)],
    );
    let mut core = host_core(true);

    core.tick_host(&host_env(1.0, 120.0, 20.0, 4.0, 4.0), &tracker);
    assert_eq!(core.song_row, 1, "bar 5 with loop_point=1 should land on row 1");
}

#[test]
fn host_sync_non_looping_end_emits_stop_sentinel() {
    // 2-row song, no loop. Bar 3 (beat 12.0) is past the end.
    let tracker = tracker_one_song(
        vec![vec![Some(0)], vec![Some(0)]],
        0,
        vec![single_pattern(4)],
    );
    let mut core = host_core(false);

    core.tick_host(&host_env(1.0, 120.0, 12.0, 4.0, 4.0), &tracker);
    assert!(core.song_ended);
    assert!(core.emit_stop_sentinel);
}

#[test]
fn host_sync_daw_seek_jumps_position() {
    // 12-row song of 4-step patterns. Start at bar 1, then jump to bar 10.
    let tracker = tracker_one_song(
        (0..12).map(|i| vec![Some(i % 4)]).collect(),
        0,
        vec![
            single_pattern(4),
            single_pattern(4),
            single_pattern(4),
            single_pattern(4),
        ],
    );
    let mut core = host_core(true);

    core.tick_host(&host_env(1.0, 120.0, 4.0, 4.0, 4.0), &tracker);
    assert_eq!(core.song_row, 1);

    let r = core.tick_host(&host_env(1.0, 120.0, 40.0, 4.0, 4.0), &tracker);
    assert_eq!(core.song_row, 10);
    assert_eq!(core.pattern_step, 0);
    assert!(r.tick_fired, "tick should fire on seek to new step");
    assert!(r.reset_fired, "reset should fire on row change");
}

#[test]
fn stop_sentinel_emits_when_non_looping_song_ends() {
    let tracker = tracker_one_song(
        vec![vec![Some(0)], vec![Some(0)]],
        0,
        vec![single_pattern(2)],
    );
    let mut core = start_core(120.0, 4, 0.5);
    core.do_loop = false;

    // Advance through all steps — 2 rows × 2 steps = 4 steps. After the 4th
    // advance past the final step the song should end and emit the stop
    // sentinel.
    for _ in 0..3 {
        assert!(core.advance_step(&tracker));
    }
    assert!(!core.advance_step(&tracker), "advance past end should return false");
    assert!(core.song_ended);
    assert!(core.emit_stop_sentinel);
}
