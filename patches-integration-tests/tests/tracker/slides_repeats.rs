//! Slide and repeat step behaviour.

use super::support::build_engine;

/// Verify that slide steps produce interpolated cv1 values.
#[test]
fn pattern_with_slides() {
    let src = r#"
pattern slide_pat {
    ch: 0.0>1.0
}

song s(c) {
    play { slide_pat }
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

/// Verify that repeat steps produce multiple triggers within a single tick.
#[test]
fn pattern_with_repeats() {
    let src = r#"
pattern rep_pat {
    ch: x*3
}

song s(c) {
    play { rep_pat }
}

patch {
    module seq: MasterSequencer(channels: [c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
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

song s(c) {
    play { rep_pat }
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

song s(c) {
    play { rep_pat }
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
