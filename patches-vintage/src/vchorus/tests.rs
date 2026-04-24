use super::*;
use crate::vchorus::core::{Mode, Variant};
use patches_core::test_support::{params, ModuleHarness};
use patches_core::{AudioEnvironment, ModuleShape};

const SR: f32 = 48_000.0;
const ENV: AudioEnvironment = AudioEnvironment {
    sample_rate: SR,
    poly_voices: 16,
    periodic_update_interval: 32,
    hosted: false,
};

fn shape() -> ModuleShape {
    ModuleShape { channels: 1, length: 0, ..Default::default() }
}

fn run_sine(h: &mut ModuleHarness, n: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR;
        let x = (std::f32::consts::TAU * 440.0 * t).sin();
        h.set_mono("in_left", x);
        h.set_mono("in_right", x);
        h.tick();
        l.push(h.read_mono("out_left"));
        r.push(h.read_mono("out_right"));
    }
    (l, r)
}

fn xcorr_lr(l: &[f32], r: &[f32]) -> f32 {
    let n = l.len() as f32;
    let ml = l.iter().sum::<f32>() / n;
    let mr = r.iter().sum::<f32>() / n;
    let mut num = 0.0_f32;
    let mut dl = 0.0_f32;
    let mut dr = 0.0_f32;
    for i in 0..l.len() {
        let a = l[i] - ml;
        let b = r[i] - mr;
        num += a * b;
        dl += a * a;
        dr += b * b;
    }
    let d = (dl * dr).sqrt();
    if d < 1.0e-9 { 0.0 } else { num / d }
}

#[test]
fn descriptor_shape() {
    let h = ModuleHarness::build::<VChorus>(&[]);
    let d = h.descriptor();
    assert_eq!(d.module_name, "VChorus");
    assert_eq!(d.inputs.len(), 4);
    assert_eq!(d.outputs.len(), 2);
}

#[test]
fn off_on_bright_bypasses_signal() {
    let mut h = ModuleHarness::build_full::<VChorus>(
        params![
            "variant" => Variant::Bright,
            "mode" => Mode::Off,
            "hiss" => 0.0_f32,
        ],
        ENV,
        shape(),
    );
    h.set_mono("in_left", 0.42);
    h.set_mono("in_right", -0.17);
    h.tick();
    // Bright + off = full bypass.
    assert!((h.read_mono("out_left") - 0.42).abs() < 1.0e-5);
    assert!((h.read_mono("out_right") + 0.17).abs() < 1.0e-5);
}

#[test]
fn hiss_silent_at_zero_and_bounded_at_one() {
    // hiss=0: silent input stays silent (steady state).
    let mut h = ModuleHarness::build_full::<VChorus>(
        params!["mode" => Mode::One, "hiss" => 0.0_f32],
        ENV,
        shape(),
    );
    h.set_mono("in_left", 0.0);
    h.set_mono("in_right", 0.0);
    for _ in 0..((SR * 0.2) as usize) {
        h.tick();
    }
    assert!(h.read_mono("out_left").abs() < 1.0e-4);

    // hiss=1: noise present but not huge.
    let mut h2 = ModuleHarness::build_full::<VChorus>(
        params!["mode" => Mode::One, "hiss" => 1.0_f32],
        ENV,
        shape(),
    );
    let mut peak = 0.0_f32;
    for _ in 0..((SR * 0.1) as usize) {
        h2.set_mono("in_left", 0.0);
        h2.set_mono("in_right", 0.0);
        h2.tick();
        peak = peak.max(h2.read_mono("out_left").abs());
    }
    // Bright hiss_floor = 0.0020 (see `VChorusCore::hiss_floor`). Uniform PRNG
    // output is in [-1, 1], so the raw noise peak is ~0.002. The reconstruction
    // low-pass softens it slightly but does not amplify — peak settles in the
    // 0.001–0.005 range. Tight upper bound of 0.02 leaves one order of
    // magnitude of headroom for run-to-run variation without masking a
    // regression where the floor is misapplied (e.g. driven by hiss=1 directly,
    // which would push peak toward 1.0).
    assert!(
        peak > 0.0 && peak < 0.02,
        "hiss peak {peak} out of expected range [0, 0.02]"
    );
}

#[test]
fn mode_both_on_bright_more_modulated_than_mode_one() {
    // FM index ≈ f_carrier * delay_depth * lfo_rate. Mode I+II on
    // bright has depth=0.2 ms, rate=9.75 Hz → index bigger than mode I
    // (depth=1.85 ms, rate=0.513 Hz) despite the tighter sweep, so L/R
    // decorrelate more under the inverse-LFO trick.
    let mut h1 = ModuleHarness::build_full::<VChorus>(
        params![
            "variant" => Variant::Bright,
            "mode" => Mode::One,
            "hiss" => 0.0_f32,
        ],
        ENV,
        shape(),
    );
    let n = (SR * 0.8) as usize;
    let (l1, r1) = run_sine(&mut h1, n);
    let c1 = xcorr_lr(&l1, &r1);

    let mut h2 = ModuleHarness::build_full::<VChorus>(
        params![
            "variant" => Variant::Bright,
            "mode" => Mode::Both,
            "hiss" => 0.0_f32,
        ],
        ENV,
        shape(),
    );
    let (l2, r2) = run_sine(&mut h2, n);
    let c2 = xcorr_lr(&l2, &r2);

    assert!(
        c2 < c1,
        "mode both should yield lower L/R correlation than mode one: both={c2}, one={c1}"
    );
}

#[test]
fn dark_variant_does_not_bypass_when_off() {
    // Dark + off: signal still traverses the BBD with zero modulation,
    // so it is *not* bit-identical to the input (unlike bright).
    let mut h = ModuleHarness::build_full::<VChorus>(
        params![
            "variant" => Variant::Dark,
            "mode" => Mode::Off,
            "hiss" => 0.0_f32,
        ],
        ENV,
        shape(),
    );
    let mut all_equal = true;
    for i in 0..((SR * 0.1) as usize) {
        let t = i as f32 / SR;
        let x = 0.5 * (std::f32::consts::TAU * 440.0 * t).sin();
        h.set_mono("in_left", x);
        h.set_mono("in_right", x);
        h.tick();
        let out = h.read_mono("out_left");
        if (out - x).abs() > 1.0e-4 {
            all_equal = false;
            break;
        }
    }
    assert!(!all_equal, "dark + off must still pass through the BBD");
}
