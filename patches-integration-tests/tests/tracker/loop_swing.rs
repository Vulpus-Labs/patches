//! Song loop point and swing timing.

use super::support::build_engine;

/// Verify that a song with `@loop` loops back to the correct row.
#[test]
fn song_loop_point() {
    let src = r#"
pattern p {
    ch: x
}

song loop_song(c) {
    play {
        p
    }
    @loop
    play {
        p
        p
    }
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: loop_song, bpm: 120, rows_per_beat: 4,
        loop: true, autostart: true, swing: 0.5
    }
    module player: PatternPlayer(channels: [ch])
    module t2a: SyncToTrigger
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> t2a.in
    t2a.out -> out.in_left
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

song s(c) {
    play {
        intro
    }
    @loop
    play {
        loop_a
        loop_b
    }
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: true, autostart: true, swing: 0.5
    }
    module player: PatternPlayer(channels: [ch])
    module t2a: SyncToTrigger
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.cv1[ch] -> out.in_left
    player.trigger[ch] -> t2a.in
    t2a.out -> out.in_right
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

/// Verify that swing produces alternating long/short tick intervals.
#[test]
fn swing_alternates_tick_durations() {
    let src = r#"
pattern p {
    ch: x x x x x x x x
}

song s(c) {
    play { p }
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.67
    }
    module player: PatternPlayer(channels: [ch])
    module t2a: SyncToTrigger
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> t2a.in
    t2a.out -> out.in_left
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
