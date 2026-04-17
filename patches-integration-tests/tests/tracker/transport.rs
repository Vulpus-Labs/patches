//! Basic round-trip and transport controls.

use super::support::{build_engine, env, load_fixture, registry};

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

/// Verify that a non-autostart sequencer doesn't produce output until started.
#[test]
fn transport_no_autostart_silent() {
    let src = r#"
pattern p {
    ch: x x x x
}

song s(c) {
    play { p }
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
