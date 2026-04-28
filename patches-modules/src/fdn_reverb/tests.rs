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
    assert_eq!(desc.inputs.len(),  5);
    assert_eq!(desc.outputs.len(), 1);
    assert_eq!(desc.inputs[0].name,  "in");
    assert_eq!(desc.inputs[1].name,  "size_cv");
    assert_eq!(desc.inputs[2].name,  "brightness_cv");
    assert_eq!(desc.inputs[3].name,  "pre_delay_cv");
    assert_eq!(desc.inputs[4].name,  "mix_cv");
    assert_eq!(desc.outputs[0].name, "out");
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
        h.disconnect_input("size_cv");
        h.disconnect_input("brightness_cv");
        h.disconnect_input("pre_delay_cv");
        h.disconnect_input("mix_cv");

        h.set_stereo("in", 1.0, 0.0);
        h.tick();
        h.set_stereo("in", 0.0, 0.0);

        let n = 32_768;
        let out: Vec<f32> = (0..n).map(|_| { h.tick(); h.read_stereo("out").0 }).collect();

        let peak = out.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
        assert!(peak.is_finite(), "character={character:?}: non-finite output");
        assert!(peak > 0.0, "character={character:?}: zero output after impulse");
        assert!(
            peak < 2.0,
            "character={character:?}: peak {peak} exceeds bounded-response limit"
        );

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
    let mut h = make_fdn(Character::Plate, 0.1, 0.5);
    h.disconnect_input("size_cv");
    h.disconnect_input("brightness_cv");
    h.disconnect_input("pre_delay_cv");
    h.disconnect_input("mix_cv");

    let dc = 0.1_f32;
    h.set_stereo("in", dc, dc);
    let outputs: Vec<f32> = (0..4096).map(|_| { h.tick(); h.read_stereo("out").0 }).collect();
    for (i, &v) in outputs.iter().enumerate() {
        assert!(v.is_finite(), "output[{i}] is not finite: {v}");
    }
    let max_out = outputs.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
    assert!(max_out > 0.0, "DC input produced no output");
    assert!(
        max_out < dc * 10.0,
        "DC input {dc} produced unbounded output {max_out}"
    );
    let tail = &outputs[outputs.len() - 256..];
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    let var = tail.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / tail.len() as f32;
    assert!(
        var < (dc * dc),
        "DC steady-state variance {var:.6} too large vs input² {:.6}",
        dc * dc
    );
}

/// With a mono-broadcast input, out_l and out_r differ (channel decorrelation
/// from orthogonal output gain vectors).
#[test]
fn stereo_output_decorrelation() {
    let mut h = make_fdn(Character::Hall, 0.5, 0.5);
    h.disconnect_input("size_cv");
    h.disconnect_input("brightness_cv");
    h.disconnect_input("pre_delay_cv");
    h.disconnect_input("mix_cv");

    // Mono-style input: identical L and R. Reverb's orthogonal output
    // gains should still produce decorrelated L/R.
    h.set_stereo("in", 0.5, 0.5);
    for _ in 0..2048 {
        h.tick();
    }
    let (l, r) = h.read_stereo("out");

    assert!(l.is_finite() && r.is_finite(), "stereo output contains NaN/inf");
    assert!(
        (l - r).abs() > 1e-6,
        "out_l ({l}) and out_r ({r}) are identical — no decorrelation"
    );
}
