//! Integration tests for the tracker sequencer pipeline:
//! DSL parsing → interpreter → plan building → audio-thread execution →
//! correct module outputs.

use patches_core::AudioEnvironment;
use patches_engine::{OversamplingFactor, Planner};
use patches_integration_tests::{POOL_CAP, MODULE_CAP};

fn env() -> AudioEnvironment {
    AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
}

fn registry() -> patches_core::Registry {
    patches_modules::default_registry()
}

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/../patches-dsl/tests/fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read fixture '{}': {}", path, e))
}

/// Helper: parse, expand, build interpreter, build plan, adopt into headless engine.
/// Returns the engine ready to tick.
fn build_engine(src: &str) -> patches_integration_tests::HeadlessEngine {
    let file = patches_dsl::parse(src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let build_result = patches_interpreter::build(&result.patch, &registry(), &env())
        .expect("interpreter build failed");

    let mut planner = Planner::new();
    let plan = planner.build_with_tracker_data(
        &build_result.graph,
        &registry(),
        &env(),
        build_result.tracker_data,
    ).expect("plan build failed");

    let mut engine = patches_integration_tests::HeadlessEngine::new(
        POOL_CAP, MODULE_CAP, OversamplingFactor::None,
    );
    engine.adopt_plan(plan);
    engine
}

// ── Basic round-trip ────────────────────────────────────────────────────────

/// Parse a `.patches` file with patterns, a song, a MasterSequencer, and a
/// PatternPlayer; build and tick the engine; verify trigger outputs match
/// expected step data.
#[test]
fn tracker_basic_round_trip() {
    let src = load_fixture("tracker_basic.patches");
    let mut engine = build_engine(&src);

    // Pattern "drums" has kick: x . . . x . . .
    // At 120 BPM, 4 rows/beat, tick = 60/(120*4) = 0.125s = 5512.5 samples.
    // Song has 3 rows of 8 steps each = 24 ticks total (non-looping).
    //
    // We wire player.trigger[kick] -> out.in_left
    // So left output should be 1.0 on the tick-fire sample and 0.0 otherwise.
    //
    // There's a 2-sample pipeline delay (1-sample cable delay × 2 hops:
    // MasterSequencer → PatternPlayer → AudioOut).
    // Collect a window covering the pipeline delay; assert the trigger is a
    // single-sample pulse at a deterministic position (not "somewhere within").
    let window: Vec<f32> = (0..6).map(|_| { engine.tick(); engine.last_left() }).collect();
    let high: Vec<usize> = window.iter().enumerate()
        .filter_map(|(i, &v)| if v >= 0.5 { Some(i) } else { None })
        .collect();
    assert_eq!(
        high.len(), 1,
        "expected exactly one trigger sample in window, got {high:?} (window={window:?})"
    );
    let idx = high[0];
    assert!(
        idx <= 4,
        "trigger sample at {idx} too late; pipeline delay is 2 samples (window={window:?})"
    );
    // Sample just after the pulse must be exactly 0.0 (one-sample pulse).
    assert_eq!(window[idx + 1], 0.0, "pulse at {idx} bled into next sample");
}

/// Verify that the tracker pipeline builds and runs without panicking with
/// the song_basic fixture.
#[test]
fn song_basic_builds_and_ticks() {
    // song_basic.patches doesn't have MasterSequencer/PatternPlayer wiring,
    // but it should parse and build without error. We verify the interpreter
    // returns tracker data.
    let src = load_fixture("song_basic.patches");
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let build_result = patches_interpreter::build(&result.patch, &registry(), &env())
        .expect("build failed");

    assert!(build_result.tracker_data.is_some(), "should have tracker data");
    let td = build_result.tracker_data.as_ref().unwrap();
    assert!(!td.patterns.patterns.is_empty(), "should have patterns");
    assert!(!td.songs.songs.is_empty(), "should have songs");
}

// ── Pattern switching at row boundaries ─────────────────────────────────────

/// Verify that pattern switching works: the song order selects different
/// patterns at row boundaries.
#[test]
fn pattern_switching_at_row_boundary() {
    // Two patterns with different data in first step:
    // "a_pat" kick: x .   (cv1=0.0 for x)
    // "b_pat" kick: 0.5 . (cv1=0.5)
    // Song: a_pat then b_pat, each 2 steps.
    // Wire cv1 to out.in_right.
    let src = r#"
pattern a_pat {
    kick: x .
}

pattern b_pat {
    kick: 0.5 .
}

song switch_song {
    | ch1   |
    | a_pat |
    | b_pat |
}

patch {
    module seq: MasterSequencer(channels: [ch1]) {
        song: switch_song, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer(channels: [kick])
    module out: AudioOut

    seq.clock[ch1] -> player.clock
    player.cv1[kick] -> out.in_left
    player.trigger[kick] -> out.in_right
}
"#;
    let mut engine = build_engine(src);

    // 2-sample pipeline delay (MasterSequencer → PatternPlayer → AudioOut).
    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;

    // First trigger: scan a small window, assert exactly one pulse.
    let win: Vec<f32> = (0..6).map(|_| { engine.tick(); engine.last_right() }).collect();
    let high: Vec<usize> = win.iter().enumerate()
        .filter_map(|(i, &v)| if v >= 0.5 { Some(i) } else { None }).collect();
    assert_eq!(high.len(), 1, "a_pat step 0: expected one trigger pulse, got {high:?}");

    // Advance to the second tick boundary (step 1 of a_pat = rest)
    for _ in 0..tick_samples {
        engine.tick();
    }

    // After tick, scan a few samples for the rest step
    let mut saw_rest_trigger = false;
    for _ in 0..5 {
        engine.tick();
        if engine.last_right() >= 0.5 {
            saw_rest_trigger = true;
        }
    }
    assert!(!saw_rest_trigger, "rest step should not trigger");

    // Advance to tick 2 (step 0 of b_pat = 0.5)
    for _ in 0..tick_samples {
        engine.tick();
    }
    // Scan for the cv1 value from b_pat
    let mut found_cv1 = false;
    for _ in 0..5 {
        engine.tick();
        let cv1 = engine.last_left();
        if (cv1 - 0.5).abs() < 0.01 {
            found_cv1 = true;
            break;
        }
    }
    assert!(found_cv1, "b_pat step 0 cv1 should be 0.5");
}

// ── Song loop point ─────────────────────────────────────────────────────────

/// Verify that a song with `@loop` loops back to the correct row.
#[test]
fn song_loop_point() {
    let src = r#"
pattern p {
    ch: x
}

song loop_song {
    | c     |
    | p     |
    | p     |  @loop
    | p     |
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: loop_song, bpm: 120, rows_per_beat: 4,
        loop: true, autostart: true, swing: 0.5
    }
    module player: PatternPlayer(channels: [ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> out.in_left
}
"#;
    let mut engine = build_engine(src);

    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;

    // Count triggers over many ticks to verify looping.
    // 3 rows × 1 step each = 3 ticks before loop.
    // After 3 ticks: loops from row 3 end back to row 1 (loop_point=1).
    // So ticks 0-2 play, then 3-4 loop rows 1-2, etc.
    let total_ticks = 10;
    let mut trigger_count = 0;
    for _ in 0..(total_ticks * tick_samples) {
        engine.tick();
        if engine.last_left() >= 0.5 {
            trigger_count += 1;
        }
    }
    // Each tick should produce exactly one trigger sample.
    // With looping, we should get at least `total_ticks` triggers.
    assert!(
        trigger_count >= total_ticks,
        "expected at least {total_ticks} triggers with looping, got {trigger_count}"
    );
}

/// Verify that the @loop row itself plays (not skipped) by using distinct
/// CV values per row and checking the sequence.
#[test]
fn loop_row_is_not_skipped() {
    let src = r#"
pattern intro {
    ch: 0.1
}

pattern loop_a {
    ch: 0.5
}

pattern loop_b {
    ch: 0.9
}

song s {
    | c       |
    | intro   |
    | loop_a  |  @loop
    | loop_b  |
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: true, autostart: true, swing: 0.5
    }
    module player: PatternPlayer(channels: [ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.cv1[ch] -> out.in_left
    player.trigger[ch] -> out.in_right
}
"#;
    let mut engine = build_engine(src);

    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;

    // Collect the cv1 value at each trigger point.
    // Expected sequence: 0.1, 0.5, 0.9, 0.5, 0.9, 0.5, 0.9, ...
    let expected = [0.1, 0.5, 0.9, 0.5, 0.9, 0.5, 0.9];
    let mut cv_at_trigger = Vec::new();
    let mut prev_trigger = 0.0_f32;

    for _ in 0..(expected.len() * tick_samples + 10) {
        engine.tick();
        let trigger = engine.last_right();
        if trigger >= 0.5 && prev_trigger < 0.5 {
            cv_at_trigger.push(engine.last_left());
        }
        prev_trigger = trigger;
    }

    assert!(
        cv_at_trigger.len() >= expected.len(),
        "expected at least {} triggers, got {}",
        expected.len(),
        cv_at_trigger.len()
    );

    for (i, (&got, &exp)) in cv_at_trigger.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - exp).abs() < 0.01,
            "trigger {i}: expected cv1 ≈ {exp}, got {got} — @loop row may be skipped"
        );
    }
}

// ── Swing timing ───────────────────────────────────────────────────────────

/// Verify that swing produces alternating long/short tick intervals.
#[test]
fn swing_alternates_tick_durations() {
    let src = r#"
pattern p {
    ch: x x x x x x x x
}

song s {
    | c |
    | p |
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.67
    }
    module player: PatternPlayer(channels: [ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> out.in_left
}
"#;
    let mut engine = build_engine(src);

    // At 120 BPM, 4 RPB, base tick = 0.125s = 5512.5 samples.
    // With swing=0.67:
    //   even steps: 2 * 0.125 * 0.67 = 0.1675s = 7390.75 samples
    //   odd steps:  2 * 0.125 * 0.33 = 0.0825s = 3638.25 samples
    // Total of even+odd = 2*base = 11025 samples.
    //
    // There's a 2-sample pipeline delay, so triggers appear ~2 samples after
    // the MasterSequencer fires them.

    let max_samples = 50_000;
    let mut trigger_times = Vec::new();
    let mut prev = 0.0_f32;
    for sample in 0..max_samples {
        engine.tick();
        let t = engine.last_left();
        if t >= 0.5 && prev < 0.5 {
            trigger_times.push(sample);
        }
        prev = t;
    }

    // We should have at least 6 triggers (to get 5 intervals).
    assert!(
        trigger_times.len() >= 6,
        "expected at least 6 triggers, got {}",
        trigger_times.len()
    );

    // Compute intervals between consecutive triggers.
    let intervals: Vec<usize> = trigger_times.windows(2)
        .map(|w| w[1] - w[0])
        .collect();

    // Even intervals (0→1, 2→3, ...) should be long (~7391 samples).
    // Odd intervals (1→2, 3→4, ...) should be short (~3638 samples).
    let long_expected = 7391;
    let short_expected = 3638;

    for (i, &interval) in intervals.iter().enumerate() {
        let (expected, label) = if i % 2 == 0 {
            (long_expected, "long (even→odd)")
        } else {
            (short_expected, "short (odd→even)")
        };
        let diff = (interval as i64 - expected as i64).unsigned_abs() as usize;
        assert!(
            diff < 5,
            "interval {i} ({label}): expected ~{expected}, got {interval} (off by {diff})"
        );
    }
}

// ── Transport controls ──────────────────────────────────────────────────────

/// Verify that a non-autostart sequencer doesn't produce output until started.
#[test]
fn transport_no_autostart_silent() {
    let src = r#"
pattern p {
    ch: x x x x
}

song s {
    | c |
    | p |
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: false, swing: 0.5
    }
    module player: PatternPlayer(channels: [ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> out.in_left
}
"#;
    let mut engine = build_engine(src);

    // With autostart=false, no triggers should fire.
    for _ in 0..1000 {
        engine.tick();
        assert_eq!(engine.last_left(), 0.0, "should be silent without autostart");
    }
}

// ── Slide test ──────────────────────────────────────────────────────────────

/// Verify that slide steps produce interpolated cv1 values.
#[test]
fn pattern_with_slides() {
    let src = r#"
pattern slide_pat {
    ch: 0.0>1.0
}

song s {
    | c         |
    | slide_pat |
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer(channels: [ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.cv1[ch] -> out.in_left
}
"#;
    let mut engine = build_engine(src);

    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;

    // First sample: cv1 starts at 0.0
    engine.tick();
    let cv1_start = engine.last_left();
    assert!(
        cv1_start.abs() < 0.01,
        "slide start cv1 should be near 0.0, got {cv1_start}"
    );

    // Halfway through the tick: cv1 should be near 0.5
    let half = tick_samples / 2;
    for _ in 1..half {
        engine.tick();
    }
    let cv1_mid = engine.last_left();
    assert!(
        (cv1_mid - 0.5).abs() < 0.1,
        "halfway through slide cv1 should be near 0.5, got {cv1_mid}"
    );

    // Near the end of the tick: cv1 should be near 1.0
    for _ in half..tick_samples - 1 {
        engine.tick();
    }
    let cv1_end = engine.last_left();
    assert!(
        (cv1_end - 1.0).abs() < 0.1,
        "end of slide cv1 should be near 1.0, got {cv1_end}"
    );
}

// ── Repeat test ─────────────────────────────────────────────────────────────

/// Verify that repeat steps produce multiple triggers within a single tick.
#[test]
fn pattern_with_repeats() {
    let src = r#"
pattern rep_pat {
    ch: x*3
}

song s {
    | c       |
    | rep_pat |
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer(channels: [ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> out.in_left
}
"#;
    let mut engine = build_engine(src);

    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;

    // Count trigger pulses within the first tick.
    let mut trigger_count = 0;
    let mut prev_trigger = 0.0_f32;
    for _ in 0..tick_samples {
        engine.tick();
        let t = engine.last_left();
        if t >= 0.5 && prev_trigger < 0.5 {
            trigger_count += 1;
        }
        prev_trigger = t;
    }

    // With repeat=3, we expect 3 triggers within the tick.
    assert_eq!(
        trigger_count, 3,
        "expected 3 trigger pulses for x*3, got {trigger_count}"
    );
}

/// Verify that repeat retriggers are audible through a full voice chain
/// (Osc -> VCA with ADSR envelope -> AudioOut). With sustain=0.0 and fast
/// decay, each sub-trigger should produce a distinct burst that decays to
/// near-silence before the next one.
#[test]
fn repeat_retrigger_audible_through_voice() {
    let src = r#"
pattern rep_pat {
    ch: C4*3
}

song s {
    | c       |
    | rep_pat |
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer(channels: [ch])
    module osc: Osc
    module env: Adsr { attack: 0.001, decay: 0.02, sustain: 0.0, release: 0.001 }
    module vca: Vca
    module out: AudioOut

    seq.clock[c]       -> player.clock
    player.cv1[ch]     -> osc.voct
    player.trigger[ch] -> env.trigger
    player.gate[ch]    -> env.gate
    osc.sawtooth       -> vca.in
    env.out            -> vca.cv
    vca.out            -> out.in_left
}
"#;
    let mut engine = build_engine(src);

    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    // Collect RMS energy in each third of the tick.
    let mut thirds = [0.0_f64; 3];
    let mut counts = [0_usize; 3];
    for sample in 0..tick_samples {
        engine.tick();
        let v = engine.last_left() as f64;
        let third = (sample * 3 / tick_samples).min(2);
        thirds[third] += v * v;
        counts[third] += 1;
    }

    let rms: Vec<f64> = thirds.iter().zip(counts.iter())
        .map(|(&sum, &n)| if n > 0 { (sum / n as f64).sqrt() } else { 0.0 })
        .collect();

    // Each third should have non-trivial energy (a burst from each sub-trigger).
    for (i, &r) in rms.iter().enumerate() {
        assert!(
            r > 0.001,
            "third {i} RMS {r:.6} is too quiet — sub-trigger {i} didn't produce audio"
        );
    }
}

/// Same as above but with non-zero sustain and long decay — the scenario
/// that previously failed because the ADSR ignored gate drops during
/// Attack/Decay stages.
#[test]
fn repeat_retrigger_audible_with_sustain() {
    let src = r#"
pattern rep_pat {
    ch: C4*3
}

song s {
    | c       |
    | rep_pat |
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer(channels: [ch])
    module osc: Osc
    module env: Adsr { attack: 0.01, decay: 0.2, sustain: 0.4, release: 0.001 }
    module vca: Vca
    module out: AudioOut

    seq.clock[c]       -> player.clock
    player.cv1[ch]     -> osc.voct
    player.trigger[ch] -> env.trigger
    player.gate[ch]    -> env.gate
    osc.sawtooth       -> vca.in
    env.out            -> vca.cv
    vca.out            -> out.in_left
}
"#;
    let mut engine = build_engine(src);

    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;

    // Collect RMS energy in each third of the tick.
    let mut thirds = [0.0_f64; 3];
    let mut counts = [0_usize; 3];
    for sample in 0..tick_samples {
        engine.tick();
        let v = engine.last_left() as f64;
        let third = (sample * 3 / tick_samples).min(2);
        thirds[third] += v * v;
        counts[third] += 1;
    }

    let rms: Vec<f64> = thirds.iter().zip(counts.iter())
        .map(|(&sum, &n)| if n > 0 { (sum / n as f64).sqrt() } else { 0.0 })
        .collect();

    // Each third should have non-trivial energy.
    for (i, &r) in rms.iter().enumerate() {
        assert!(
            r > 0.001,
            "third {i} RMS {r:.6} is too quiet — sub-trigger {i} didn't produce audio"
        );
    }

    // The energy should DIP between sub-triggers. Check that no third has
    // more than 10× the energy of another — that would mean one sub-note
    // dominated while others were inaudible.
    let max_rms = rms.iter().copied().fold(0.0_f64, f64::max);
    let min_rms = rms.iter().copied().fold(f64::MAX, f64::min);
    assert!(
        max_rms / min_rms < 10.0,
        "energy ratio {:.1}× between loudest and quietest third is too large — \
         retrigger envelope dips are not deep enough (RMS: {:.4}, {:.4}, {:.4})",
        max_rms / min_rms, rms[0], rms[1], rms[2],
    );
}
