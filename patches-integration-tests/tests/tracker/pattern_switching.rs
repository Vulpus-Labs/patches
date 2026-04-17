//! Pattern switching at row boundaries.

use super::support::build_engine;

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

song switch_song(ch1) {
    play {
        a_pat
        b_pat
    }
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
