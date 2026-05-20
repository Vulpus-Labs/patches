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
    module seq: MasterSequencer([c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer([ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.cv1[ch] -> out.in
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
    module seq: MasterSequencer([c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer([ch])
    module t2a: SyncToTrigger
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> t2a.in
    t2a.out -> out.in
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

// ── E152 tie-spread rolls (epic 0942) ────────────────────────────────

/// `x*3 _` — three sub-triggers spread across two ticks. The
/// absorbed-tie tick fires no fresh trigger of its own; sub-triggers
/// land at offsets `0`, `2T/3`, `4T/3` (T = anchor tick samples).
#[test]
fn pattern_with_tie_spread_x3_tilde() {
    let src = r#"
pattern roll_pat {
    ch: x*3 _ . .
}

song s(c) {
    play { roll_pat }
}

patch {
    module seq: MasterSequencer([c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer([ch])
    module t2a: SyncToTrigger
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> t2a.in
    t2a.out -> out.in
}
"#;
    let mut engine = build_engine(src);

    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    let two_ticks = 2 * tick_samples;
    let expected_interval = (2 * tick_samples) as f32 / 3.0;

    let mut trigger_offsets = Vec::new();
    let mut prev_trigger = 0.0_f32;
    for sample in 0..two_ticks {
        engine.tick();
        let t = engine.last_left();
        if t >= 0.5 && prev_trigger < 0.5 {
            trigger_offsets.push(sample);
        }
        prev_trigger = t;
    }

    assert_eq!(
        trigger_offsets.len(),
        3,
        "x*3 _ should fire 3 triggers across two ticks (one absorbed by the roll), got {trigger_offsets:?}"
    );

    // Spacing tolerance: ±2 samples — the integer floor of the
    // schedule's fractional sample offsets.
    for (i, &off) in trigger_offsets.iter().enumerate() {
        let want = (i as f32 * expected_interval).round() as i64;
        let diff = (off as i64 - want).abs();
        assert!(
            diff <= 2,
            "trigger {i}: sample {off} drifts {diff} from expected {want}"
        );
    }

    // Last sub-trigger must sit inside the two-tick span — no overrun.
    assert!(
        *trigger_offsets.last().unwrap() < two_ticks,
        "last sub-trigger overran the span ({} >= {two_ticks})",
        trigger_offsets.last().unwrap()
    );
}

/// `x*5 _ _` — quintuplet over three ticks. Verifies the longer span
/// formula and that the trailing rest ticks fire no fresh triggers.
#[test]
fn pattern_with_tie_spread_x5_tilde_tilde() {
    let src = r#"
pattern roll_pat {
    ch: x*5 _ _ . .
}

song s(c) {
    play { roll_pat }
}

patch {
    module seq: MasterSequencer([c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer([ch])
    module t2a: SyncToTrigger
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> t2a.in
    t2a.out -> out.in
}
"#;
    let mut engine = build_engine(src);

    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    let three_ticks = 3 * tick_samples;
    let expected_interval = three_ticks as f32 / 5.0;

    let mut trigger_offsets = Vec::new();
    let mut prev_trigger = 0.0_f32;
    for sample in 0..three_ticks {
        engine.tick();
        let t = engine.last_left();
        if t >= 0.5 && prev_trigger < 0.5 {
            trigger_offsets.push(sample);
        }
        prev_trigger = t;
    }

    assert_eq!(
        trigger_offsets.len(),
        5,
        "x*5 _ _ should fire 5 triggers across three ticks, got {trigger_offsets:?}"
    );

    for (i, &off) in trigger_offsets.iter().enumerate() {
        let want = (i as f32 * expected_interval).round() as i64;
        let diff = (off as i64 - want).abs();
        assert!(
            diff <= 2,
            "trigger {i}: sample {off} drifts {diff} from expected {want}"
        );
    }
}

/// Plain `_` after a non-repeat note keeps its sustain meaning — the
/// gate stays high across the tie tick and no new trigger fires.
/// Regression guard for E152: the new tie interpretation must NOT
/// hijack ties without a preceding `*N` anchor.
#[test]
fn pattern_with_plain_tie_sustains_unchanged() {
    let src = r#"
pattern sustain_pat {
    ch: A3 _ . .
}

song s(c) {
    play { sustain_pat }
}

patch {
    module seq: MasterSequencer([c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer([ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.gate[ch] -> out.in
}
"#;
    let mut engine = build_engine(src);

    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    let two_ticks = 2 * tick_samples;

    // Gate must stay high across both ticks (no drop between anchor
    // and tie). Only an initial rising edge — no second one.
    let mut rising_edges = 0;
    let mut prev = 0.0_f32;
    let mut tie_tick_gate_low = false;
    for sample in 0..two_ticks {
        engine.tick();
        let g = engine.last_left();
        if g >= 0.5 && prev < 0.5 {
            rising_edges += 1;
        }
        if sample == tick_samples + tick_samples / 2 && g < 0.5 {
            tie_tick_gate_low = true;
        }
        prev = g;
    }
    assert_eq!(rising_edges, 1, "sustain tie should not re-fire gate");
    assert!(
        !tie_tick_gate_low,
        "sustain tie must hold gate high through the tie tick"
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
    module seq: MasterSequencer([c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer([ch])
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
    vca.out            -> out.in
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
    module seq: MasterSequencer([c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }
    module player: PatternPlayer([ch])
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
    vca.out            -> out.in
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

// ── ADR 0077 / ticket 0946: unified step-event grammar ───────────────

/// Helper: drive a single-channel pattern, return the sample offsets at
/// which `player.trigger` produced a one-sample pulse (via SyncToTrigger).
fn run_triggers_only(pattern_body: &str, n_ticks: usize) -> Vec<usize> {
    let src = format!(
        r#"
pattern p {{
    ch: {pattern_body}
}}

song s(c) {{
    play {{ p }}
}}

patch {{
    module seq: MasterSequencer([c]) {{
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }}
    module player: PatternPlayer([ch])
    module t2a: SyncToTrigger
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> t2a.in
    t2a.out -> out.in
}}
"#
    );
    let mut engine = build_engine(&src);
    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    let total = tick_samples * n_ticks;
    let mut offsets = Vec::new();
    let mut prev = 0.0_f32;
    for s in 0..total {
        engine.tick();
        let t = engine.last_left();
        if t >= 0.5 && prev < 0.5 {
            offsets.push(s);
        }
        prev = t;
    }
    offsets
}

/// Helper: drive a single-channel pattern, return per-sample cv1 across
/// `n_ticks` ticks (player.cv1 cabled to AudioOut.in).
fn run_cv1(pattern_body: &str, n_ticks: usize) -> Vec<f32> {
    let src = format!(
        r#"
pattern p {{
    ch: {pattern_body}
}}

song s(c) {{
    play {{ p }}
}}

patch {{
    module seq: MasterSequencer([c]) {{
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }}
    module player: PatternPlayer([ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.cv1[ch] -> out.in
}}
"#
    );
    let mut engine = build_engine(&src);
    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    let total = tick_samples * n_ticks;
    let mut cv1 = Vec::with_capacity(total);
    for _ in 0..total {
        engine.tick();
        cv1.push(engine.last_left());
    }
    cv1
}

/// `E4> _ /G4` — slide opens with a trigger on tick 0, ramps across two
/// ticks, lands at G4 at the tick-3 boundary, held without retrigger on
/// tick 3.
#[test]
fn slide_two_ticks_no_retrigger() {
    // E4 = 4/12, G4 = 7/12.
    let triggers = run_triggers_only("E4> _ /G4 .", 4);
    assert_eq!(triggers, vec![0], "exactly one trigger at sample 0");

    let cv1 = run_cv1("E4> _ /G4 .", 4);
    let tick_samples = cv1.len() / 4;
    // Patches v/oct: C0 = 0.0, each octave = 1.0.
    let e4 = 4.0 + 4.0 / 12.0;
    let g4 = 4.0 + 7.0 / 12.0;
    assert!((cv1[0] - e4).abs() < 0.01, "start at E4, got {}", cv1[0]);
    // End of tick 2 (= start of tick 3 boundary): should be near G4.
    let landing = cv1[2 * tick_samples - 1];
    assert!(
        (landing - g4).abs() < 0.05,
        "should land near G4 by end of tick 2, got {landing}"
    );
    // Tick 3: held at G4 (no ramp, no retrigger).
    let held = cv1[3 * tick_samples - 1];
    assert!(
        (held - g4).abs() < 0.01,
        "held at G4 through tick 3, got {held}"
    );
}

/// `E4> _ >G4` — slide opens with a trigger on tick 0, ramps continuously
/// across ALL three ticks, ending at G4 inside tick 2. One trigger only.
#[test]
fn slide_three_ticks_full_ramp() {
    let triggers = run_triggers_only("E4> _ >G4 .", 4);
    assert_eq!(triggers, vec![0]);

    let cv1 = run_cv1("E4> _ >G4 .", 4);
    let tick_samples = cv1.len() / 4;
    // Patches v/oct: C0 = 0.0, each octave = 1.0.
    let e4 = 4.0 + 4.0 / 12.0;
    let g4 = 4.0 + 7.0 / 12.0;
    // Roughly midway through the 3-tick ramp (start of tick 2) should
    // be near 2/3 of the way from E4 to G4.
    let mid = cv1[(3 * tick_samples) / 2];
    let want_mid = e4 + 0.5 * (g4 - e4);
    assert!(
        (mid - want_mid).abs() < 0.05,
        "midway through 3-tick ramp should be near {want_mid}, got {mid}"
    );
    // End of tick 2 (last sample of slide): near G4.
    let end_of_slide = cv1[3 * tick_samples - 1];
    assert!(
        (end_of_slide - g4).abs() < 0.05,
        "should land near G4 by end of tick 2, got {end_of_slide}"
    );
}

/// `E4> _ G4` — slide closes at boundary with a fresh trigger on tick 2.
#[test]
fn slide_two_ticks_then_retrigger() {
    let triggers = run_triggers_only("E4> _ G4 .", 4);
    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    assert_eq!(
        triggers.len(),
        2,
        "expected two triggers (open + fresh G4 close), got {triggers:?}"
    );
    assert_eq!(triggers[0], 0);
    let drift = (triggers[1] as i64 - 2 * tick_samples as i64).abs();
    assert!(
        drift <= 1,
        "second trigger ~start of tick 2 (the G4 close cell), got sample {} (drift {drift})",
        triggers[1],
    );
}

/// `E4 /G4 _` — bare E4 triggers on tick 0; /G4 jumps cv to G4 at start
/// of tick 1 with no trigger; tick 2 holds G4.
#[test]
fn step_cv_no_trigger() {
    let triggers = run_triggers_only("E4 /G4 _ .", 4);
    assert_eq!(triggers, vec![0], "only the E4 cell triggers");

    let cv1 = run_cv1("E4 /G4 _ .", 4);
    let tick_samples = cv1.len() / 4;
    // Patches v/oct: C0 = 0.0, each octave = 1.0.
    let e4 = 4.0 + 4.0 / 12.0;
    let g4 = 4.0 + 7.0 / 12.0;
    // Tick 0: E4.
    assert!((cv1[tick_samples / 2] - e4).abs() < 0.01);
    // Tick 1: snap to G4 at boundary.
    assert!(
        (cv1[tick_samples + 1] - g4).abs() < 0.01,
        "cv1 snaps to G4 at tick 1 start, got {}",
        cv1[tick_samples + 1]
    );
    // Tick 2 (held by `_`): still at G4.
    let tick2_mid = cv1[2 * tick_samples + tick_samples / 2];
    assert!(
        (tick2_mid - g4).abs() < 0.01,
        "_ holds G4 through tick 2, got {tick2_mid}"
    );
}

/// `E4 _ >_ /G4` — hold E4 across ticks 0+1, slide opens via `>_` on
/// tick 2, closes at boundary at /G4 on tick 3.
#[test]
fn late_slide_open_mid_row() {
    let triggers = run_triggers_only("E4 _ >_ /G4", 4);
    assert_eq!(triggers, vec![0], "only the initial E4 triggers");

    let cv1 = run_cv1("E4 _ >_ /G4", 4);
    let tick_samples = cv1.len() / 4;
    // Patches v/oct: C0 = 0.0, each octave = 1.0.
    let e4 = 4.0 + 4.0 / 12.0;
    let g4 = 4.0 + 7.0 / 12.0;
    // Ticks 0+1: held at E4.
    assert!((cv1[tick_samples / 2] - e4).abs() < 0.01);
    assert!((cv1[tick_samples + tick_samples / 2] - e4).abs() < 0.01);
    // Tick 3: held at G4 (slide closed at boundary, no trigger).
    let tick3_mid = cv1[3 * tick_samples + tick_samples / 2];
    assert!(
        (tick3_mid - g4).abs() < 0.01,
        "tick 3 should hold G4 (slide closed at boundary), got {tick3_mid}"
    );
}

// ── Ticket 0948: cv2 ramps across multi-cell slides ─────────────────

/// Drive a single-channel pattern with cv2 cabled to `out.in`, return
/// per-sample cv2 across `n_ticks` ticks.
fn run_cv2(pattern_body: &str, n_ticks: usize) -> Vec<f32> {
    let src = format!(
        r#"
pattern p {{
    ch: {pattern_body}
}}

song s(c) {{
    play {{ p }}
}}

patch {{
    module seq: MasterSequencer([c]) {{
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }}
    module player: PatternPlayer([ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.cv2[ch] -> out.in
}}
"#
    );
    let mut engine = build_engine(&src);
    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    let total = tick_samples * n_ticks;
    let mut cv2 = Vec::with_capacity(total);
    for _ in 0..total {
        engine.tick();
        cv2.push(engine.last_left());
    }
    cv2
}

/// `C4:0.5> _ >C4:1.0` — cv1 stays at C4 (start = end), cv2 ramps
/// 0.5 → 1.0 continuously across all three ticks.
#[test]
fn slide_ramps_cv1_and_cv2_simultaneously() {
    let cv2 = run_cv2("C4:0.5> _ >C4:1.0 .", 4);
    let tick_samples = cv2.len() / 4;

    // Tick 1 start: cv2 = 0.5.
    assert!((cv2[1] - 0.5).abs() < 0.01, "tick 1 start cv2≈0.5, got {}", cv2[1]);
    // Mid-ramp (middle of tick 2): cv2 ≈ 0.75.
    let mid = cv2[(3 * tick_samples) / 2];
    assert!((mid - 0.75).abs() < 0.05, "mid-ramp cv2≈0.75, got {mid}");
    // End of tick 2 (last slide sample, close-in-tick): cv2 ≈ 1.0.
    let end_of_slide = cv2[3 * tick_samples - 1];
    assert!((end_of_slide - 1.0).abs() < 0.05, "end-of-slide cv2≈1.0, got {end_of_slide}");
    // Tick 3 (rest): gate drops but cv2 holds at the last value (1.0).
    // Rests don't reset cv — that's consistent with analog convention
    // and with cv1's behaviour on a rest tick.
    let rest_mid = cv2[3 * tick_samples + tick_samples / 2];
    assert!((rest_mid - 1.0).abs() < 0.01, "rest tick cv2 holds at 1.0, got {rest_mid}");
}

// ── Ticket 0947: ADR 0077 continuation absorption goldens ───────────

/// Drive a single-channel pattern with `player.gate` cabled to
/// `AudioOut.in`, return per-sample gate across `n_ticks` ticks.
fn run_gate(pattern_body: &str, n_ticks: usize) -> Vec<f32> {
    let src = format!(
        r#"
pattern p {{
    ch: {pattern_body}
}}

song s(c) {{
    play {{ p }}
}}

patch {{
    module seq: MasterSequencer([c]) {{
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.5
    }}
    module player: PatternPlayer([ch])
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.gate[ch] -> out.in
}}
"#
    );
    let mut engine = build_engine(&src);
    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    let total = tick_samples * n_ticks;
    let mut gate = Vec::with_capacity(total);
    for _ in 0..total {
        engine.tick();
        gate.push(engine.last_left());
    }
    gate
}

/// `E4 /G4` — two-tick figure (ADR 0077 § "Continuation absorption"):
/// trigger at sample 0 only; cv1 snaps to G4 at the tick-1→tick-2
/// boundary; gate held high across both ticks (no drop on the `/G4`).
#[test]
fn step_to_two_tick_figure() {
    let triggers = run_triggers_only("E4 /G4", 2);
    assert_eq!(triggers, vec![0], "exactly one trigger at sample 0");

    let cv1 = run_cv1("E4 /G4", 2);
    let tick_samples = cv1.len() / 2;
    let e4 = 4.0 + 4.0 / 12.0;
    let g4 = 4.0 + 7.0 / 12.0;
    assert!(
        (cv1[tick_samples / 2] - e4).abs() < 0.01,
        "tick 1 mid cv1≈E4, got {}",
        cv1[tick_samples / 2]
    );
    assert!(
        (cv1[tick_samples + 1] - g4).abs() < 0.01,
        "snap to G4 at the 1→2 boundary, got {}",
        cv1[tick_samples + 1]
    );

    let gate = run_gate("E4 /G4", 2);
    let mut rising_edges = 0;
    let mut prev = 0.0_f32;
    for &g in &gate {
        if g >= 0.5 && prev < 0.5 {
            rising_edges += 1;
        }
        prev = g;
    }
    assert_eq!(rising_edges, 1, "gate rises once for the E4 trigger");
    assert!(
        gate[tick_samples + tick_samples / 2] >= 0.5,
        "gate held high through the /G4 tick"
    );
}

/// `E4 _ /G4` — three-tick figure: trigger at sample 0; sustain `_` on
/// tick 2; cv1 snaps to G4 at the tick-2→tick-3 boundary.
#[test]
fn step_to_after_sustain_three_tick_figure() {
    let triggers = run_triggers_only("E4 _ /G4", 3);
    assert_eq!(triggers, vec![0], "exactly one trigger at sample 0");

    let cv1 = run_cv1("E4 _ /G4", 3);
    let tick_samples = cv1.len() / 3;
    let e4 = 4.0 + 4.0 / 12.0;
    let g4 = 4.0 + 7.0 / 12.0;
    assert!(
        (cv1[tick_samples + tick_samples / 2] - e4).abs() < 0.01,
        "tick 2 (sustain) holds E4, got {}",
        cv1[tick_samples + tick_samples / 2]
    );
    assert!(
        (cv1[2 * tick_samples + 1] - g4).abs() < 0.01,
        "snap to G4 at the 2→3 boundary, got {}",
        cv1[2 * tick_samples + 1]
    );
}

/// `E4> _ G4` — three ticks: trigger at sample 0; ramp across ticks
/// 1+2 ending exactly at G4 at the tick-2→tick-3 boundary; fresh
/// trigger at start of tick 3. Tightens the assertion in
/// `slide_two_ticks_then_retrigger` with the ADR 0077 sample-offset
/// expectations.
#[test]
fn slide_two_ticks_then_retrigger_offsets() {
    let triggers = run_triggers_only("E4> _ G4", 3);
    let tick_samples = (44100.0 * 60.0 / (120.0 * 4.0)) as usize;
    assert_eq!(triggers.len(), 2, "two triggers (open + close), got {triggers:?}");
    assert_eq!(triggers[0], 0, "first trigger at sample 0");
    let drift = (triggers[1] as i64 - 2 * tick_samples as i64).abs();
    assert!(
        drift <= 1,
        "second trigger sits at sample {} (drift {} from {})",
        triggers[1],
        drift,
        2 * tick_samples,
    );

    let cv1 = run_cv1("E4> _ G4", 3);
    let e4 = 4.0 + 4.0 / 12.0;
    let g4 = 4.0 + 7.0 / 12.0;
    let want_mid = e4 + 0.5 * (g4 - e4);
    let mid = cv1[tick_samples];
    assert!(
        (mid - want_mid).abs() < 0.05,
        "mid of 2-tick ramp (start of tick 2) near {want_mid}, got {mid}"
    );
    let landing = cv1[2 * tick_samples - 1];
    assert!(
        (landing - g4).abs() < 0.05,
        "ramp lands at G4 by end of tick 2, got {landing}"
    );
    let tick3_mid = cv1[2 * tick_samples + tick_samples / 2];
    assert!(
        (tick3_mid - g4).abs() < 0.01,
        "tick 3 holds G4 from the fresh trigger, got {tick3_mid}"
    );
}

/// `E4 _ >_ /G4` — four ticks: trigger at sample 0; flat ticks 1+2;
/// ramp tick 3 (opened by `>_`); G4 held at tick 4. Tightens the
/// assertion in `late_slide_open_mid_row` with explicit per-tick
/// sample-offset cv1 expectations.
#[test]
fn late_slide_open_four_tick_figure_with_offsets() {
    let triggers = run_triggers_only("E4 _ >_ /G4", 4);
    assert_eq!(triggers, vec![0], "exactly one trigger at sample 0");

    let cv1 = run_cv1("E4 _ >_ /G4", 4);
    let tick_samples = cv1.len() / 4;
    let e4 = 4.0 + 4.0 / 12.0;
    let g4 = 4.0 + 7.0 / 12.0;
    // Tick 1 + 2: flat at E4.
    assert!(
        (cv1[tick_samples / 2] - e4).abs() < 0.01,
        "tick 1 mid E4, got {}",
        cv1[tick_samples / 2]
    );
    assert!(
        (cv1[tick_samples + tick_samples / 2] - e4).abs() < 0.01,
        "tick 2 mid (held E4) got {}",
        cv1[tick_samples + tick_samples / 2]
    );
    // Tick 3 mid: midway through the one-tick ramp E4→G4.
    let want_mid = e4 + 0.5 * (g4 - e4);
    let mid = cv1[2 * tick_samples + tick_samples / 2];
    assert!(
        (mid - want_mid).abs() < 0.05,
        "tick 3 mid (ramp) near {want_mid}, got {mid}"
    );
    // Tick 4 mid: held at G4 (slide closed at the 3→4 boundary).
    let tick4_mid = cv1[3 * tick_samples + tick_samples / 2];
    assert!(
        (tick4_mid - g4).abs() < 0.01,
        "tick 4 mid (post-close) holds G4, got {tick4_mid}"
    );
}

/// Swung `x*3 _` golden — exercises the per-tick sub-event schedule
/// from ticket 0945. The schedule emits sub-events at
/// `t_k = (k/N) · S` ticks; the runtime resolves each to a sample
/// offset against the *current* tick's swung duration, not the
/// anchor's. With swing=0.7 the anchor (on-beat) tick is 1.4 × base
/// and the absorbed `_` (off-beat) tick is 0.6 × base.
#[test]
fn swung_tie_spread_respects_per_tick_durations() {
    let src = r#"
pattern roll_pat {
    ch: x*3 _ . .
}

song s(c) {
    play { roll_pat }
}

patch {
    module seq: MasterSequencer([c]) {
        song: s, bpm: 120, rows_per_beat: 4,
        loop: false, autostart: true, swing: 0.7
    }
    module player: PatternPlayer([ch])
    module t2a: SyncToTrigger
    module out: AudioOut

    seq.clock[c] -> player.clock
    player.trigger[ch] -> t2a.in
    t2a.out -> out.in
}
"#;
    let mut engine = build_engine(src);

    // bpm=120, rows_per_beat=4 → base tick = sr * 60 / (120*4)
    // = 5512.5 samples. swing=0.7 → long_tick = 1.4 × base ≈ 7718,
    // short_tick = 0.6 × base ≈ 3308. Anchor (step 0, on-beat) is
    // long; absorbed `_` (step 1, off-beat) is short.
    let base: f64 = 44100.0 * 60.0 / (120.0 * 4.0);
    let long_tick = base * 1.4;
    let short_tick = base * 0.6;

    // ADR 0077: schedule positions are k/N × S ticks (S=2, N=3).
    //  k=0 → 0      ticks → sample 0
    //  k=1 → 2/3    ticks → 2/3 of long_tick ≈ 5145
    //  k=2 → 4/3    ticks → long_tick + 1/3 × short_tick ≈ 8820
    let expected = [
        0.0,
        (2.0 / 3.0) * long_tick,
        long_tick + (1.0 / 3.0) * short_tick,
    ];

    let span_samples = (long_tick + short_tick) as usize + long_tick as usize;
    let mut trigger_offsets = Vec::new();
    let mut prev = 0.0_f32;
    for sample in 0..span_samples {
        engine.tick();
        let t = engine.last_left();
        if t >= 0.5 && prev < 0.5 {
            trigger_offsets.push(sample);
        }
        prev = t;
    }

    assert_eq!(
        trigger_offsets.len(),
        3,
        "expected 3 sub-triggers across swung span, got {trigger_offsets:?}",
    );
    for (i, (&off, &want)) in trigger_offsets.iter().zip(expected.iter()).enumerate() {
        let drift = (off as f64 - want).abs();
        assert!(
            drift <= 8.0,
            "sub-trigger {i}: sample {off} drifts {drift:.1} from per-tick \
             swung expectation {want:.1}",
        );
    }
}

/// `C4:0.5> _ >G4` — cv1 ramps C4→G4; cv2 holds at 0.5 throughout
/// (close cell has no `:cv2`, runtime falls back to open's cv2).
#[test]
fn slide_ramps_cv1_with_cv2_held() {
    let cv2 = run_cv2("C4:0.5> _ >G4 .", 4);
    let tick_samples = cv2.len() / 4;

    // Tick 1 start: cv2 = 0.5.
    assert!((cv2[1] - 0.5).abs() < 0.01);
    // Mid-ramp tick 2: cv2 still 0.5 (no cv2 ramp).
    let mid = cv2[(3 * tick_samples) / 2];
    assert!((mid - 0.5).abs() < 0.01, "mid-ramp cv2 holds at 0.5, got {mid}");
    // End of tick 2: still 0.5.
    let end_of_slide = cv2[3 * tick_samples - 1];
    assert!((end_of_slide - 0.5).abs() < 0.01, "end-of-slide cv2 holds at 0.5, got {end_of_slide}");
}
