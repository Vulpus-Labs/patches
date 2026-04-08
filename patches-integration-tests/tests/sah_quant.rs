//! Integration tests for `Sah`, `PolySah`, `Quant`, and `PolyQuant`.

use patches_core::parameter_map::ParameterValue;
use patches_core::test_support::{ModuleHarness, params};
use patches_modules::{PolySah, PolyQuant, Quant, Sah};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn quant_with_notes(notes: &[&str]) -> ModuleHarness {
    let arr: Vec<String> = notes.iter().map(|s| s.to_string()).collect();
    ModuleHarness::build::<Quant>(&[("notes", ParameterValue::Array(arr.into()))])
}

fn poly_quant_with_notes(notes: &[&str]) -> ModuleHarness {
    let arr: Vec<String> = notes.iter().map(|s| s.to_string()).collect();
    ModuleHarness::build::<PolyQuant>(&[("notes", ParameterValue::Array(arr.into()))])
}

// ── Sah tests ────────────────────────────────────────────────────────────────

/// Clock fires at sample 1; verify `out` holds the same latched value for all
/// subsequent samples even as `in` changes.
#[test]
fn sah_holds_on_trigger() {
    let mut h = ModuleHarness::build::<Sah>(params![]);
    // Sample 0: trig low, in = 0.123 — nothing latched yet
    h.set_mono("trig", 0.0);
    h.set_mono("in", 0.123);
    h.tick();
    assert_eq!(h.read_mono("out"), 0.0); // initial held = 0.0

    // Sample 1: rising edge — latches 0.456
    h.set_mono("trig", 1.0);
    h.set_mono("in", 0.456);
    h.tick();
    let latched = h.read_mono("out");
    assert!((latched - 0.456).abs() < 1e-6, "expected 0.456, got {latched}");

    // Samples 2–9: trig stays low, in changes — out must hold
    h.set_mono("trig", 0.0);
    for step in 0..8 {
        h.set_mono("in", step as f32 * 0.1);
        h.tick();
        let v = h.read_mono("out");
        assert!((v - latched).abs() < 1e-6, "sample {step}: expected {latched}, got {v}");
    }
}

/// Two triggers at samples 1 and 10; verify the held value changes between them.
#[test]
fn sah_updates_on_each_trigger() {
    let mut h = ModuleHarness::build::<Sah>(params![]);

    // First trigger: latch 0.3
    h.set_mono("trig", 0.0);
    h.set_mono("in", 0.0);
    h.tick();

    h.set_mono("trig", 1.0);
    h.set_mono("in", 0.3);
    h.tick();
    let first_held = h.read_mono("out");
    assert!((first_held - 0.3).abs() < 1e-6);

    // Gate stays low for 8 samples
    h.set_mono("trig", 0.0);
    for _ in 0..8 {
        h.tick();
    }

    // Second trigger at sample 10: latch 0.9
    h.set_mono("trig", 1.0);
    h.set_mono("in", 0.9);
    h.tick();
    let second_held = h.read_mono("out");
    assert!((second_held - 0.9).abs() < 1e-6);
    assert!(
        (second_held - first_held).abs() > 1e-3,
        "second trigger should produce a different held value"
    );
}

/// PolySah with a single trigger: all 16 voices must be latched simultaneously.
#[test]
fn poly_sah_holds_all_voices() {
    let mut h = ModuleHarness::build::<PolySah>(params![]);

    let voices: [f32; 16] = std::array::from_fn(|i| (i as f32 + 1.0) * 0.05);
    h.set_poly("in", voices);
    h.set_mono("trig", 0.0);
    h.tick();
    // Before trigger: all held = 0.0
    assert!(h.read_poly("out").iter().all(|&v| v == 0.0));

    // Rising edge: latch all voices
    h.set_mono("trig", 1.0);
    h.tick();
    let out = h.read_poly("out");
    for i in 0..16 {
        assert!(
            (out[i] - voices[i]).abs() < 1e-6,
            "voice {i}: expected {}, got {}", voices[i], out[i]
        );
    }

    // Gate goes low, change inputs — held values must not change
    h.set_mono("trig", 0.0);
    h.set_poly("in", [0.0; 16]);
    h.tick();
    let out_after = h.read_poly("out");
    for i in 0..16 {
        assert!(
            (out_after[i] - voices[i]).abs() < 1e-6,
            "voice {i} changed after trig went low: expected {}, got {}", voices[i], out_after[i]
        );
    }
}

// ── Quant tests ───────────────────────────────────────────────────────────────

/// Feed several known inputs to `Quant` with `notes = ["0", "7"]` and verify
/// outputs snap to the expected semitone.
#[test]
fn quant_snaps_to_nearest_note() {
    // Notes: 0 (root) and 7 (fifth)
    let mut h = quant_with_notes(&["0", "7"]);

    // 0.0 voct → semitone 0 → root (0)
    h.set_mono("in", 0.0);
    h.tick();
    assert!((h.read_mono("out") - 0.0).abs() < 1e-5, "0.0 voct → 0.0");

    // 7/12 voct → semitone 7 → fifth exactly
    h.set_mono("in", 7.0 / 12.0);
    h.tick();
    assert!((h.read_mono("out") - 7.0 / 12.0).abs() < 1e-5, "7/12 voct → 7/12");

    // 3/12 voct → semitone 3 → closer to 0 than to 7
    h.set_mono("in", 3.0 / 12.0);
    h.tick();
    assert!((h.read_mono("out") - 0.0).abs() < 1e-5, "3/12 voct → 0.0 (closer to root)");

    // 5/12 voct → semitone 5 → closer to 7 than to 0
    h.set_mono("in", 5.0 / 12.0);
    h.tick();
    assert!((h.read_mono("out") - 7.0 / 12.0).abs() < 1e-5, "5/12 voct → 7/12 (closer to fifth)");
}

/// `trig_out` must go high exactly on the sample where the quantised pitch
/// changes, and be 0.0 the very next sample.
#[test]
fn quant_trig_out_fires_on_change() {
    let mut h = quant_with_notes(&["0", "7"]);

    // Initial: input 0.0, quantised 0.0, same as last_quantised (0.0) → no trig
    h.set_mono("in", 0.0);
    h.tick();
    assert_eq!(h.read_mono("trig_out"), 0.0, "no change on first tick at 0.0");

    // Change to fifth
    h.set_mono("in", 7.0 / 12.0);
    h.tick();
    assert_eq!(h.read_mono("trig_out"), 1.0, "trig_out fires on pitch change");

    // Same input → no further change
    h.tick();
    assert_eq!(h.read_mono("trig_out"), 0.0, "trig_out clears next sample");
}

/// Same input on repeated ticks must not produce spurious `trig_out` pulses after the first.
#[test]
fn quant_no_spurious_trig_out() {
    let mut h = quant_with_notes(&["0", "7"]);
    h.set_mono("in", 7.0 / 12.0);
    h.tick(); // first quantise: may or may not fire depending on initial state

    // Repeat the same input many times — trig_out must stay 0.0
    for i in 0..16 {
        h.tick();
        assert_eq!(
            h.read_mono("trig_out"), 0.0,
            "spurious trig_out on sample {i}"
        );
    }
}

/// `out = centre + (quantised_voct * scale)`.
#[test]
fn quant_centre_and_scale() {
    let arr = vec!["0".to_string()];
    let mut h = ModuleHarness::build::<Quant>(&[
        ("notes", ParameterValue::Array(arr.into())),
        ("centre", ParameterValue::Float(1.0)),
        ("scale", ParameterValue::Float(0.5)),
    ]);
    h.set_mono("in", 0.0);
    h.tick();
    // quantised_voct = 0.0; out = 1.0 + 0.0 * 0.5 = 1.0
    assert!((h.read_mono("out") - 1.0).abs() < 1e-5, "expected 1.0, got {}", h.read_mono("out"));
}

// ── PolyQuant tests ───────────────────────────────────────────────────────────

/// Two voices with inputs that quantise to different notes; each voice's `trig_out`
/// slot fires independently and at the correct time.
#[test]
fn poly_quant_per_voice_trig_out() {
    let mut h = poly_quant_with_notes(&["0", "7"]);

    // Initial tick: all voices at 0.0 → no change from initial last_quantised (0.0)
    h.set_poly("in", [0.0; 16]);
    h.tick();
    let trig = h.read_poly("trig_out");
    assert_eq!(trig[0], 0.0, "voice 0: no change expected");
    assert_eq!(trig[1], 0.0, "voice 1: no change expected");

    // Move voice 1 to the fifth, leave voice 0 at root
    let mut input = [0.0f32; 16];
    input[1] = 7.0 / 12.0;
    h.set_poly("in", input);
    h.tick();
    let trig = h.read_poly("trig_out");
    assert_eq!(trig[0], 0.0, "voice 0: should not fire (no change)");
    assert_eq!(trig[1], 1.0, "voice 1: should fire (changed to fifth)");

    // Next tick: no change → both should be 0.0
    h.tick();
    let trig = h.read_poly("trig_out");
    assert_eq!(trig[0], 0.0, "voice 0: still no change");
    assert_eq!(trig[1], 0.0, "voice 1: trig_out must clear");

    // Move voice 0 to fifth, leave voice 1 at fifth (no change for voice 1)
    let mut input2 = [0.0f32; 16];
    input2[0] = 7.0 / 12.0;
    input2[1] = 7.0 / 12.0;
    h.set_poly("in", input2);
    h.tick();
    let trig = h.read_poly("trig_out");
    assert_eq!(trig[0], 1.0, "voice 0: fires on change to fifth");
    assert_eq!(trig[1], 0.0, "voice 1: no change (already at fifth)");
}
