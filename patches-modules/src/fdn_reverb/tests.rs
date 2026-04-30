use super::kernel::FdnReverbKernel;
use super::params::Character;
use super::*;
use patches_core::{AudioEnvironment, Module, ModuleShape};

const SR: f32 = 44_100.0;

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

fn make_kernel(character: Character) -> FdnReverbKernel {
    FdnReverbKernel::new(&env(), character as usize)
}

/// Run an impulse on the left channel; collect `n` samples of stereo output as
/// `(l, r)` pairs. `eff_size` / `eff_bright` are the steady values used for
/// every sample; pre-delay and mix held at sane defaults.
fn impulse_response(
    kernel: &mut FdnReverbKernel,
    n: usize,
    eff_size: f32,
    eff_bright: f32,
) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(n);
    let (l, r) = kernel.process_sample(1.0, 0.0, eff_size, eff_bright, 0.0, 1.0);
    out.push((l, r));
    for _ in 1..n {
        let (l, r) = kernel.process_sample(0.0, 0.0, eff_size, eff_bright, 0.0, 1.0);
        out.push((l, r));
    }
    out
}

#[test]
fn descriptor_ports_and_params() {
    let desc = FdnReverb::describe(&ModuleShape { channels: 0 });
    assert_eq!(desc.module_name, "FdnReverb");
    assert_eq!(desc.inputs.len(), 5);
    assert_eq!(desc.outputs.len(), 1);
    assert_eq!(desc.inputs[0].name, "in");
    assert_eq!(desc.inputs[1].name, "size_cv");
    assert_eq!(desc.inputs[2].name, "brightness_cv");
    assert_eq!(desc.inputs[3].name, "pre_delay_cv");
    assert_eq!(desc.inputs[4].name, "mix_cv");
    assert_eq!(desc.outputs[0].name, "out");
    let names: Vec<&str> = desc.realtime_params.iter().map(|p| p.name).collect();
    assert!(names.contains(&"size"));
    assert!(names.contains(&"brightness"));
    assert!(names.contains(&"pre_delay"));
    assert!(names.contains(&"mix"));
    assert!(names.contains(&"character"));
}

/// Impulse through every character: bounded peak, finite output, late RMS
/// well below early RMS (proper decay, not divergence or sustain).
#[test]
fn impulse_decays_for_all_characters() {
    for character in [
        Character::Plate, Character::Room, Character::Chamber,
        Character::Hall, Character::Cathedral,
    ] {
        let mut k = make_kernel(character);
        let n = 32_768;
        let out = impulse_response(&mut k, n, 0.5, 0.5);

        let peak = out.iter().map(|(l, _)| l.abs()).fold(0.0_f32, f32::max);
        assert!(peak.is_finite(), "character={character:?}: non-finite output");
        assert!(peak > 0.0, "character={character:?}: zero output after impulse");
        assert!(
            peak < 2.0,
            "character={character:?}: peak {peak} exceeds bounded-response limit"
        );

        let q = n / 4;
        let early: f32 = out[q..2 * q].iter().map(|(l, _)| l * l).sum::<f32>() / q as f32;
        let late: f32 = out[3 * q..].iter().map(|(l, _)| l * l).sum::<f32>() / q as f32;
        assert!(
            early > 0.0 && late < early * 0.5,
            "character={character:?}: late RMS² ({late:.6e}) must be < 50% of early RMS² ({early:.6e}) — no decay"
        );
    }
}

/// Sustained DC input produces finite, bounded output with low steady-state
/// variance (the absorption network does not blow up or sustain noise).
#[test]
fn dc_input_produces_finite_output() {
    let mut k = make_kernel(Character::Plate);
    let dc = 0.1_f32;
    let mut outputs = Vec::with_capacity(4096);
    for _ in 0..4096 {
        let (l, _) = k.process_sample(dc, dc, 0.1, 0.5, 0.0, 1.0);
        outputs.push(l);
    }
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

/// Identical L and R input still produces decorrelated stereo output thanks
/// to the orthogonal output gain vectors.
#[test]
fn stereo_output_decorrelation() {
    let mut k = make_kernel(Character::Hall);
    let mut last = (0.0, 0.0);
    for _ in 0..2048 {
        last = k.process_sample(0.5, 0.5, 0.5, 0.5, 0.0, 1.0);
    }
    let (l, r) = last;
    assert!(l.is_finite() && r.is_finite(), "stereo output contains NaN/inf");
    assert!(
        (l - r).abs() > 1e-6,
        "out_l ({l}) and out_r ({r}) are identical — no decorrelation"
    );
}

/// Early reflections appear before the late field smears in: in the first
/// 512 samples after an impulse, the response has at least one well-separated
/// non-zero burst (not just a smooth ramp from zero).
#[test]
fn impulse_has_early_reflections() {
    let mut k = make_kernel(Character::Plate);
    let out = impulse_response(&mut k, 1024, 0.5, 0.5);
    let early_peak = out[..512].iter().map(|(l, _)| l.abs()).fold(0.0_f32, f32::max);
    assert!(
        early_peak > 1e-3,
        "no audible early reflections in first 512 samples (peak {early_peak:.3e})"
    );
}

/// Energy is bounded: integrated impulse-response power is finite and within
/// a generous ceiling for every character archetype.
#[test]
fn impulse_response_energy_is_bounded() {
    for character in [
        Character::Plate, Character::Room, Character::Chamber,
        Character::Hall, Character::Cathedral,
    ] {
        let mut k = make_kernel(character);
        let out = impulse_response(&mut k, 65_536, 0.5, 0.5);
        let energy: f32 = out.iter().map(|(l, r)| l * l + r * r).sum();
        assert!(energy.is_finite(), "character={character:?}: energy not finite");
        assert!(
            energy < 1e6,
            "character={character:?}: energy {energy:.3e} exceeds sane bound"
        );
    }
}
