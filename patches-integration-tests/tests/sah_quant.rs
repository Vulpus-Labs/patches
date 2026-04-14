//! Integration tests for `Sah`, `PolySah`, `Quant`, and `PolyQuant`.

use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::test_support::{ModuleHarness, params};
use patches_core::ModuleShape;
use patches_modules::{PolySah, PolyQuant, Quant, Sah};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn shape(n: usize) -> ModuleShape {
    ModuleShape { channels: n, length: 0, ..Default::default() }
}

/// Build a ParameterMap with per-channel `pitch[i]` v/oct values (C0 = 0.0,
/// C1 = 1.0, each semitone = 1/12).
fn pitch_map(pitches_voct: &[f32]) -> ParameterMap {
    let mut map = ParameterMap::new();
    for (i, &p) in pitches_voct.iter().enumerate() {
        map.insert_param("pitch".to_string(), i, ParameterValue::Float(p));
    }
    map
}

fn quant_with_pitches(pitches_voct: &[f32]) -> ModuleHarness {
    let mut h = ModuleHarness::build_with_shape::<Quant>(&[], shape(pitches_voct.len()));
    h.update_params_map(&pitch_map(pitches_voct));
    h
}

fn poly_quant_with_pitches(pitches_voct: &[f32]) -> ModuleHarness {
    let mut h = ModuleHarness::build_with_shape::<PolyQuant>(&[], shape(pitches_voct.len()));
    h.update_params_map(&pitch_map(pitches_voct));
    h
}

// ── Sah tests ────────────────────────────────────────────────────────────────

#[test]
fn sah_holds_on_trigger() {
    let mut h = ModuleHarness::build::<Sah>(params![]);
    h.set_mono("trig", 0.0);
    h.set_mono("in", 0.123);
    h.tick();
    assert_eq!(h.read_mono("out"), 0.0);

    h.set_mono("trig", 1.0);
    h.set_mono("in", 0.456);
    h.tick();
    let latched = h.read_mono("out");
    assert!((latched - 0.456).abs() < 1e-6, "expected 0.456, got {latched}");

    h.set_mono("trig", 0.0);
    for step in 0..8 {
        h.set_mono("in", step as f32 * 0.1);
        h.tick();
        let v = h.read_mono("out");
        assert!((v - latched).abs() < 1e-6, "sample {step}: expected {latched}, got {v}");
    }
}

#[test]
fn sah_updates_on_each_trigger() {
    let mut h = ModuleHarness::build::<Sah>(params![]);

    h.set_mono("trig", 0.0);
    h.set_mono("in", 0.0);
    h.tick();

    h.set_mono("trig", 1.0);
    h.set_mono("in", 0.3);
    h.tick();
    let first_held = h.read_mono("out");
    assert!((first_held - 0.3).abs() < 1e-6);

    h.set_mono("trig", 0.0);
    for _ in 0..8 {
        h.tick();
    }

    h.set_mono("trig", 1.0);
    h.set_mono("in", 0.9);
    h.tick();
    let second_held = h.read_mono("out");
    assert!((second_held - 0.9).abs() < 1e-6);
    assert!((second_held - first_held).abs() > 1e-3);
}

#[test]
fn poly_sah_holds_all_voices() {
    let mut h = ModuleHarness::build::<PolySah>(params![]);

    let voices: [f32; 16] = std::array::from_fn(|i| (i as f32 + 1.0) * 0.05);
    h.set_poly("in", voices);
    h.set_mono("trig", 0.0);
    h.tick();
    assert!(h.read_poly("out").iter().all(|&v| v == 0.0));

    h.set_mono("trig", 1.0);
    h.tick();
    let out = h.read_poly("out");
    for i in 0..16 {
        assert!((out[i] - voices[i]).abs() < 1e-6);
    }

    h.set_mono("trig", 0.0);
    h.set_poly("in", [0.0; 16]);
    h.tick();
    let out_after = h.read_poly("out");
    for i in 0..16 {
        assert!((out_after[i] - voices[i]).abs() < 1e-6);
    }
}

// ── Quant tests ───────────────────────────────────────────────────────────────

/// Two-note scale: root (C0) and fifth (G0 = 7/12 v/oct).
#[test]
fn quant_snaps_to_nearest_note() {
    let mut h = quant_with_pitches(&[0.0, 7.0 / 12.0]);

    h.set_mono("in", 0.0);
    h.tick();
    assert!((h.read_mono("out") - 0.0).abs() < 1e-5);

    h.set_mono("in", 7.0 / 12.0);
    h.tick();
    assert!((h.read_mono("out") - 7.0 / 12.0).abs() < 1e-5);

    h.set_mono("in", 3.0 / 12.0);
    h.tick();
    assert!((h.read_mono("out") - 0.0).abs() < 1e-5);

    h.set_mono("in", 5.0 / 12.0);
    h.tick();
    assert!((h.read_mono("out") - 7.0 / 12.0).abs() < 1e-5);
}

#[test]
fn quant_trig_out_fires_on_change() {
    let mut h = quant_with_pitches(&[0.0, 7.0 / 12.0]);

    h.set_mono("in", 0.0);
    h.tick();
    assert_eq!(h.read_mono("trig_out"), 0.0);

    h.set_mono("in", 7.0 / 12.0);
    h.tick();
    assert_eq!(h.read_mono("trig_out"), 1.0);

    h.tick();
    assert_eq!(h.read_mono("trig_out"), 0.0);
}

#[test]
fn quant_no_spurious_trig_out() {
    let mut h = quant_with_pitches(&[0.0, 7.0 / 12.0]);
    h.set_mono("in", 7.0 / 12.0);
    h.tick();

    for i in 0..16 {
        h.tick();
        assert_eq!(h.read_mono("trig_out"), 0.0, "spurious trig_out on sample {i}");
    }
}

/// `out = centre + (quantised_voct * scale)` with a single-note scale at C0.
#[test]
fn quant_centre_and_scale() {
    let mut h = ModuleHarness::build_with_shape::<Quant>(
        &[
            ("centre", ParameterValue::Float(1.0)),
            ("scale", ParameterValue::Float(0.5)),
        ],
        shape(1),
    );
    h.set_mono("in", 0.0);
    h.tick();
    assert!((h.read_mono("out") - 1.0).abs() < 1e-5);
}

// ── PolyQuant tests ───────────────────────────────────────────────────────────

#[test]
fn poly_quant_per_voice_trig_out() {
    let mut h = poly_quant_with_pitches(&[0.0, 7.0 / 12.0]);

    h.set_poly("in", [0.0; 16]);
    h.tick();
    let trig = h.read_poly("trig_out");
    assert_eq!(trig[0], 0.0);
    assert_eq!(trig[1], 0.0);

    let mut input = [0.0f32; 16];
    input[1] = 7.0 / 12.0;
    h.set_poly("in", input);
    h.tick();
    let trig = h.read_poly("trig_out");
    assert_eq!(trig[0], 0.0);
    assert_eq!(trig[1], 1.0);

    h.tick();
    let trig = h.read_poly("trig_out");
    assert_eq!(trig[0], 0.0);
    assert_eq!(trig[1], 0.0);

    let mut input2 = [0.0f32; 16];
    input2[0] = 7.0 / 12.0;
    input2[1] = 7.0 / 12.0;
    h.set_poly("in", input2);
    h.tick();
    let trig = h.read_poly("trig_out");
    assert_eq!(trig[0], 1.0);
    assert_eq!(trig[1], 0.0);
}
