use super::*;
use super::params::Character;
use patches_core::test_support::{ModuleHarness, params};
use patches_core::{AudioEnvironment, Module, ModuleShape};

const SR: f32 = 44_100.0;

fn make_fdn(character: Character, size: f32, brightness: f32) -> ModuleHarness {
    ModuleHarness::build_with_env::<FdnReverb>(
        params!["size" => size, "brightness" => brightness, "character" => character],
        AudioEnvironment { sample_rate: SR, poly_voices: 16, periodic_update_interval: 32, hosted: false },
    )
}

#[test]
fn descriptor_ports_and_params() {
    let desc = FdnReverb::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
    assert_eq!(desc.module_name, "FdnReverb");
    assert_eq!(desc.inputs.len(),  6);
    assert_eq!(desc.outputs.len(), 2);
    assert_eq!(desc.inputs[0].name,  "in_left");
    assert_eq!(desc.inputs[1].name,  "in_right");
    assert_eq!(desc.inputs[2].name,  "size_cv");
    assert_eq!(desc.inputs[3].name,  "brightness_cv");
    assert_eq!(desc.inputs[4].name,  "pre_delay_cv");
    assert_eq!(desc.inputs[5].name,  "mix_cv");
    assert_eq!(desc.outputs[0].name, "out_left");
    assert_eq!(desc.outputs[1].name, "out_right");
    let names: Vec<&str> = desc.parameters.iter().map(|p| p.name).collect();
    assert!(names.contains(&"size"));
    assert!(names.contains(&"brightness"));
    assert!(names.contains(&"pre_delay"));
    assert!(names.contains(&"mix"));
    assert!(names.contains(&"character"));
}

/// An impulse through every character: output stays bounded, is non-zero,
/// and the late tail has lower RMS than the early tail (proper decay,
/// not divergence or sustain).
#[test]
fn impulse_decays_for_all_characters() {
    for character in [Character::Plate, Character::Room, Character::Chamber, Character::Hall, Character::Cathedral] {
        let mut h = make_fdn(character, 0.5, 0.5);
        h.disconnect_input("in_right");
        h.disconnect_input("size_cv");
        h.disconnect_input("brightness_cv");
        h.disconnect_input("pre_delay_cv");
        h.disconnect_input("mix_cv");
        h.disconnect_output("out_right");

        h.set_mono("in_left", 1.0);
        h.tick();
        h.set_mono("in_left", 0.0);

        // 32k samples ≈ 0.74 s — enough for cathedral pre-delay plus
        // multiple delay-line passes; long enough to see clear decay.
        let n = 32_768;
        let out: Vec<f32> = (0..n).map(|_| { h.tick(); h.read_mono("out_left") }).collect();

        let peak = out.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
        assert!(peak.is_finite(), "character={character:?}: non-finite output");
        assert!(peak > 0.0, "character={character:?}: zero output after impulse");
        // Bounded response: with mix=0.5 and a unit impulse, output must
        // not exceed unity by more than a small headroom; runaway feedback
        // would blow past this.
        assert!(
            peak < 2.0,
            "character={character:?}: peak {peak} exceeds bounded-response limit"
        );

        // Decay check: RMS of the last quarter must be measurably smaller
        // than RMS of the first quarter after the pre-delay region.
        let q = n / 4;
        let early: f32 = out[q..2 * q].iter().map(|v| v * v).sum::<f32>() / q as f32;
        let late: f32 = out[3 * q..].iter().map(|v| v * v).sum::<f32>() / q as f32;
        assert!(
            early > 0.0 && late < early * 0.5,
            "character={character:?}: late RMS² ({late:.6e}) must be < 50% of early RMS² ({early:.6e}) — no decay"
        );
    }
}

/// A sustained DC input produces finite, non-zero output after settling.
#[test]
fn dc_input_produces_finite_output() {
    // Use plate (short delays, fastest settling) at small size.
    let mut h = make_fdn(Character::Plate, 0.1, 0.5);
    h.disconnect_input("in_right");
    h.disconnect_input("size_cv");
    h.disconnect_input("brightness_cv");
    h.disconnect_input("pre_delay_cv");
    h.disconnect_input("mix_cv");
    h.disconnect_output("out_right");

    let dc = 0.1_f32;
    h.set_mono("in_left", dc);
    let outputs: Vec<f32> = (0..4096).map(|_| { h.tick(); h.read_mono("out_left") }).collect();
    for (i, &v) in outputs.iter().enumerate() {
        assert!(v.is_finite(), "output[{i}] is not finite: {v}");
    }
    let max_out = outputs.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
    assert!(max_out > 0.0, "DC input produced no output");
    // A passive reverb driven by DC=0.1 must not amplify beyond a small
    // multiple of the input — guards against unstable feedback gain.
    assert!(
        max_out < dc * 10.0,
        "DC input {dc} produced unbounded output {max_out}"
    );
    // Steady state: last 256 samples should have small variance compared
    // to a comparable input excursion (system has settled, not oscillating
    // wildly).
    let tail = &outputs[outputs.len() - 256..];
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    let var = tail.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / tail.len() as f32;
    assert!(
        var < (dc * dc),
        "DC steady-state variance {var:.6} too large vs input² {:.6}",
        dc * dc
    );
}

/// In mono mode (out_r disconnected), out_r's pool slot is never written.
#[test]
fn mono_mode_out_r_unchanged() {
    let mut h = make_fdn(Character::Hall, 0.5, 0.5);
    h.disconnect_input("in_right");
    h.disconnect_input("size_cv");
    h.disconnect_input("brightness_cv");
    h.disconnect_output("out_right");

    // Seed pool with a sentinel value — if out_r is written, it will change.
    h.init_pool(patches_core::CableValue::Mono(99.0));

    h.set_mono("in_left", 1.0);
    for _ in 0..64 {
        h.tick();
    }

    // out_r slot should still hold the sentinel (99.0).
    let out_r_val = h.read_mono("out_right");
    // After disconnect the port is not connected, so reads return the pool sentinel.
    // The precise check: the out_r cable slot must not have been written by the module.
    // Since we seeded with 99.0 and the module should skip out_r in mono mode, it stays 99.0.
    assert!(
        (out_r_val - 99.0).abs() < 1e-5,
        "out_r was written in mono mode: {out_r_val}"
    );
}

/// In stereo mode with mono input, out_l and out_r differ (channel decorrelation).
#[test]
fn stereo_output_decorrelation() {
    let mut h = make_fdn(Character::Hall, 0.5, 0.5);
    h.disconnect_input("in_right");
    h.disconnect_input("size_cv");
    h.disconnect_input("brightness_cv");
    h.disconnect_input("pre_delay_cv");
    h.disconnect_input("mix_cv");
    // Keep out_r connected; set_ports will set stereo_out = true.

    // Run enough samples for the reverb to build up.
    h.set_mono("in_left", 0.5);
    for _ in 0..2048 {
        h.tick();
    }
    let l = h.read_mono("out_left");
    let r = h.read_mono("out_right");

    assert!(l.is_finite() && r.is_finite(), "stereo output contains NaN/inf");
    // L and R should differ due to orthogonal output gain vectors.
    assert!(
        (l - r).abs() > 1e-6,
        "out_l ({l}) and out_r ({r}) are identical — no decorrelation"
    );
}
