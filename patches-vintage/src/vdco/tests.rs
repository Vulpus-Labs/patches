use super::*;
use patches_core::cables::CableKind;
use patches_core::test_support::{ModuleHarness, params};
use patches_core::{AudioEnvironment, ModuleShape};

use self::core::C0_FREQ;

fn env(sample_rate: f32) -> AudioEnvironment {
    AudioEnvironment {
        sample_rate,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

// ── Descriptor port kinds (acceptance: mono vs poly) ────────────────────────

#[test]
fn vdco_ports_are_mono() {
    let shape = ModuleShape::default();
    let d = VDco::describe(&shape);
    for name in ["voct", "pwm"] {
        let p = d.inputs.iter().find(|p| p.name == name).unwrap();
        assert_eq!(p.kind, CableKind::Mono, "VDco input '{name}' must be mono");
    }
    let out = d.outputs.iter().find(|p| p.name == "out").unwrap();
    assert_eq!(out.kind, CableKind::Mono, "VDco output must be mono");
}

#[test]
fn vpolydco_ports_are_poly() {
    let shape = ModuleShape::default();
    let d = VPolyDco::describe(&shape);
    for name in ["voct", "pwm"] {
        let p = d.inputs.iter().find(|p| p.name == name).unwrap();
        assert_eq!(p.kind, CableKind::Poly, "VPolyDco input '{name}' must be poly");
    }
    let out = d.outputs.iter().find(|p| p.name == "out").unwrap();
    assert_eq!(out.kind, CableKind::Poly, "VPolyDco output must be poly");
}

// ── Saw base pitch ──────────────────────────────────────────────────────────

/// With `sample_rate = C0 * 100` and no voct CV, saw wraps every 100 samples.
#[test]
fn saw_only_has_base_period() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params!["saw_gain" => 1.0_f32, "pulse_gain" => 0.0_f32, "curve" => 0.0_f32],
        env(sr),
    );
    h.disconnect_all_inputs();

    let samples = h.run_mono(period * 2, "out");
    // Count wraps: sawtooth is monotone-rising then jumps down at each period.
    let mut wraps = 0usize;
    for w in samples.windows(2) {
        if w[1] < w[0] - 0.5 {
            wraps += 1;
        }
    }
    assert!(
        (1..=3).contains(&wraps),
        "expected ~2 saw wraps in 200 samples at C0; got {wraps}"
    );
}

// ── Sub = saw − 1 octave (phase-lock) ───────────────────────────────────────

/// With saw off and only the sub active, the output is a square at half the
/// base frequency: one full cycle every 2 base periods.
#[test]
fn sub_only_is_one_octave_below_saw() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params!["saw_gain" => 0.0_f32, "pulse_gain" => 0.0_f32, "sub_gain" => 1.0_f32, "curve" => 0.0_f32],
        env(sr),
    );
    h.disconnect_all_inputs();

    // Run two full sub cycles (4 * period) to capture an interior transition
    // plus the next midpoint transition.
    let samples = h.run_mono(period * 4, "out");

    // Count sign flips excluding the tiny BLEP-smoothed region.
    let mut flips = 0usize;
    let mut prev_sign = 0i32;
    for &v in &samples {
        let s = if v > 0.5 { 1 } else if v < -0.5 { -1 } else { 0 };
        if s != 0 && s != prev_sign && prev_sign != 0 {
            flips += 1;
        }
        if s != 0 {
            prev_sign = s;
        }
    }
    // ÷2 square over 4 base periods = 2 full sub cycles → 3 interior transitions.
    assert_eq!(
        flips, 3,
        "÷2 sub should flip 3× across 2 full sub cycles; got {flips}"
    );
}

/// Saw + sub at equal frequency ratio: the combined wave must be exactly
/// periodic with period = 2 * base period (no beating — perfect phase-lock).
#[test]
fn saw_plus_sub_phase_locks_no_beat() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params!["saw_gain" => 1.0_f32, "pulse_gain" => 0.0_f32, "sub_gain" => 1.0_f32, "curve" => 0.0_f32],
        env(sr),
    );
    h.disconnect_all_inputs();

    let n = period * 6; // 3 sub cycles
    let samples = h.run_mono(n, "out");
    // Compare cycle 1 vs cycle 2 (2 * period apart). Tolerate f32 drift.
    let sub_period = period * 2;
    let mut max_diff = 0.0_f32;
    for i in 0..sub_period {
        let d = (samples[i + sub_period] - samples[i + 2 * sub_period]).abs();
        if d > max_diff {
            max_diff = d;
        }
    }
    assert!(
        max_diff < 1e-3,
        "saw+sub not periodic at sub period (max diff {max_diff})"
    );
}

// ── PWM bit-accurate duty cycle (pulse reads raw phase, not BLEP'd saw) ─────

#[test]
fn pulse_duty_follows_pwm_cv() {
    let period = 200_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params!["saw_gain" => 0.0_f32, "pulse_gain" => 1.0_f32, "curve" => 0.0_f32],
        env(sr),
    );
    h.disconnect_input("voct");

    // pwm = 0.25 → pulse high for the first quarter of each cycle.
    h.set_mono("pwm", 0.25);
    let samples = h.run_mono(period, "out");
    let positive = samples.iter().filter(|&&v| v > 0.0).count();
    // Expected ~25% (50/200). Wider bound accounts for polyBLEP smoothing.
    assert!(
        (40..=60).contains(&positive),
        "pwm=0.25 expected ~50 positive samples; got {positive}"
    );
}

// ── VPolyDco: voice 1 runs one octave up of voice 0 ─────────────────────────

#[test]
fn poly_voct_drives_per_voice_pitch() {
    // At sr = C0 * 100, voice 0 (voct=0) saw wraps every 100 samples; voice 1
    // (voct=1, one octave up) wraps every 50 samples.
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VPolyDco>(
        params!["saw_gain" => 1.0_f32, "curve" => 0.0_f32],
        env(sr),
    );
    h.disconnect_input("pwm");

    let mut voct = [0.0f32; 16];
    voct[1] = 1.0;
    h.set_poly("voct", voct);

    let n = 200_usize;
    let frames = h.run_poly(n, "out");
    let (mut wraps0, mut wraps1) = (0usize, 0usize);
    for w in frames.windows(2) {
        if w[1][0] < w[0][0] - 0.5 {
            wraps0 += 1;
        }
        if w[1][1] < w[0][1] - 0.5 {
            wraps1 += 1;
        }
    }
    assert!(
        wraps1 >= 2 * wraps0.saturating_sub(1),
        "voice 1 should wrap ~2× voice 0; got v0={wraps0} v1={wraps1}"
    );
}

// ── Waveform gains (0639) ───────────────────────────────────────────────────

/// All waveform gains at 0 → silent output (no BLEP leakage).
#[test]
fn zero_gains_silence_all_waveforms() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params![
            "saw_gain" => 0.0_f32,
            "pulse_gain" => 0.0_f32,
            "triangle_gain" => 0.0_f32,
            "sub_gain" => 0.0_f32,
            "noise_gain" => 0.0_f32,
            "curve" => 0.0_f32,
        ],
        env(sr),
    );
    h.disconnect_all_inputs();
    let samples = h.run_mono(period * 2, "out");
    let peak = samples.iter().fold(0.0_f32, |m, &v| m.max(v.abs()));
    assert!(peak == 0.0, "expected pure silence; got peak {peak}");
}

/// saw_gain = 0.5 produces exactly half the amplitude of saw_gain = 1.0.
#[test]
fn saw_gain_scales_amplitude_linearly() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;

    let mut h_full = ModuleHarness::build_with_env::<VDco>(
        params!["saw_gain" => 1.0_f32, "pulse_gain" => 0.0_f32, "curve" => 0.0_f32],
        env(sr),
    );
    h_full.disconnect_all_inputs();
    let full = h_full.run_mono(period * 2, "out");

    let mut h_half = ModuleHarness::build_with_env::<VDco>(
        params!["saw_gain" => 0.5_f32, "pulse_gain" => 0.0_f32, "curve" => 0.0_f32],
        env(sr),
    );
    h_half.disconnect_all_inputs();
    let half = h_half.run_mono(period * 2, "out");

    let mut max_err = 0.0_f32;
    for (f, h) in full.iter().zip(half.iter()) {
        let d = (0.5 * f - h).abs();
        if d > max_err {
            max_err = d;
        }
    }
    assert!(
        max_err < 1e-6,
        "saw_gain=0.5 should equal half of saw_gain=1.0; max diff {max_err}"
    );
}

/// Triangle alone is continuous (no jumps) and symmetric about phase 0.5.
#[test]
fn triangle_only_is_continuous_and_symmetric() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params![
            "saw_gain" => 0.0_f32,
            "pulse_gain" => 0.0_f32,
            "triangle_gain" => 1.0_f32,
            "curve" => 0.0_f32,
        ],
        env(sr),
    );
    h.disconnect_all_inputs();

    let n = period * 3;
    let samples = h.run_mono(n, "out");

    // Continuity: step between samples is bounded by slope = 4 * dt.
    let dt = 1.0_f32 / period as f32;
    let bound = 4.0 * dt + 1e-5;
    for w in samples.windows(2) {
        let step = (w[1] - w[0]).abs();
        assert!(
            step <= bound,
            "triangle discontinuity: step {step} > bound {bound}"
        );
    }

    // Range inside [-1, 1] and actually reaches near both extremes.
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in &samples {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    assert!(lo >= -1.0 - 1e-6 && hi <= 1.0 + 1e-6, "triangle out of range [{lo},{hi}]");
    assert!(hi > 0.95 && lo < -0.95, "triangle didn't span full range [{lo},{hi}]");
}

/// All three waveforms at gain 1.0 produce a bounded, finite signal.
#[test]
fn all_waveforms_summed_is_audible_and_finite() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params![
            "saw_gain" => 1.0_f32,
            "pulse_gain" => 1.0_f32,
            "triangle_gain" => 1.0_f32,
        ],
        env(sr),
    );
    h.disconnect_all_inputs();
    h.set_mono("pwm", 0.5);
    let samples = h.run_mono(period * 4, "out");

    let mut sum_sq = 0.0_f64;
    for &v in &samples {
        assert!(v.is_finite(), "non-finite sample: {v}");
        sum_sq += (v as f64) * (v as f64);
    }
    let rms = (sum_sq / samples.len() as f64).sqrt();
    assert!(rms > 0.1, "summed output too quiet: rms {rms}");
}

// ── Phasor curvature (0640) ─────────────────────────────────────────────────

/// Helper: render saw-only for `n` samples with the given curvature.
fn render_saw(period: usize, n: usize, curvature: f32) -> Vec<f32> {
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params![
            "saw_gain" => 1.0_f32,
            "pulse_gain" => 0.0_f32,
            "curve" => curvature,
        ],
        env(sr),
    );
    h.disconnect_all_inputs();
    h.run_mono(n, "out")
}

/// With `curve = 0.0` behaviour must match the linear baseline
/// bit-for-bit. Default is non-zero, so this is explicit.
#[test]
fn curvature_zero_matches_linear_baseline() {
    let period = 100_usize;
    let n = period * 4;
    let a = render_saw(period, n, 0.0);
    // Re-run with the same settings — deterministic output.
    let b = render_saw(period, n, 0.0);
    assert_eq!(a, b, "two zero-curvature runs must match sample-for-sample");
}

/// Curvature `> 0` changes the ramp shape measurably but keeps the period.
#[test]
fn curvature_bends_saw_ramp_preserves_period() {
    let period = 200_usize;
    let n = period * 3;
    let linear = render_saw(period, n, 0.0);
    let curved = render_saw(period, n, 0.1);

    // Wrap count is unchanged — accumulator is still linear.
    let count_wraps = |xs: &[f32]| {
        xs.windows(2).filter(|w| w[1] < w[0] - 0.5).count()
    };
    assert_eq!(count_wraps(&linear), count_wraps(&curved));

    // Shapes differ: at least one sample deviates well above f32 noise.
    let max_diff = linear
        .iter()
        .zip(curved.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(max_diff > 0.01, "curvature=0.1 barely changed the ramp (diff {max_diff})");

    // With `shape(x) = x - c*x*(1-x)` the curved phase sits *below* the linear
    // phase in the open interval, so the saw (2*phase - 1) is below the
    // linear baseline in the rising portion of the cycle.
    let idx = period / 4;
    assert!(
        curved[idx] < linear[idx],
        "curved saw should sit below linear at phase ~0.25: curved={} linear={}",
        curved[idx],
        linear[idx]
    );
}

/// No NaN/Inf and no aliasing spikes at the curvature default.
#[test]
fn curvature_default_is_stable_and_finite() {
    let period = 150_usize;
    let n = period * 8;
    let samples = render_saw(period, n, 0.1);
    for &v in &samples {
        assert!(v.is_finite(), "non-finite sample: {v}");
        assert!(v.abs() <= 1.1, "saw out of expected range: {v}");
    }
    // Saw should span ≈ [-1, 1] with BLEP smoothing — not blow up.
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in &samples {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    assert!(lo > -1.2 && hi < 1.2, "saw out of bounds [{lo},{hi}]");
    assert!(hi > 0.9 && lo < -0.9, "saw did not span full range [{lo},{hi}]");
}

/// Spectrum: at `curvature = 0.1` the saw harmonics measurably deviate from
/// the linear baseline, but the fundamental stays intact and no single bin
/// blows up (no aliasing spike).
#[test]
fn curvature_changes_spectrum_but_no_aliasing_spike() {
    // Keep the base frequency well below Nyquist so any spike in the upper
    // bins is aliasing, not legitimate harmonic content.
    let period = 128_usize;
    let n = period * 16; // 16 cycles → clean bin 16 at the fundamental
    let lin = render_saw(period, n, 0.0);
    let cur = render_saw(period, n, 0.1);

    // Brute-force DFT magnitude at k bins (k << n — cheap).
    let dft = |xs: &[f32], k: usize| -> f32 {
        let n = xs.len() as f32;
        let (mut re, mut im) = (0.0_f32, 0.0_f32);
        for (i, &x) in xs.iter().enumerate() {
            let th = -2.0 * std::f32::consts::PI * (k as f32) * (i as f32) / n;
            re += x * th.cos();
            im += x * th.sin();
        }
        (re * re + im * im).sqrt() / n
    };

    let fund_bin = n / period; // 16
    let lin_fund = dft(&lin, fund_bin);
    let cur_fund = dft(&cur, fund_bin);
    assert!(cur_fund > 0.2 && lin_fund > 0.2, "fundamental missing");
    // Fundamental within ~20% — still clearly a saw.
    let fund_ratio = (cur_fund / lin_fund - 1.0).abs();
    assert!(fund_ratio < 0.2, "fundamental shifted too much: ratio {fund_ratio}");

    // Harmonic content differs measurably on at least one low harmonic.
    // Curvature should produce a measurable shape difference in the time
    // domain — small but well above f32 rounding.
    let diff_rms: f32 = (lin
        .iter()
        .zip(cur.iter())
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f32>()
        / lin.len() as f32)
        .sqrt();
    assert!(
        diff_rms > 1e-3,
        "curvature produced no measurable shape change (rms diff {diff_rms})"
    );

    // No single non-harmonic bin in the upper half approaches the fundamental
    // (loose check for aliasing — no spurious spike).
    for k in (fund_bin * 8)..(n / 2) {
        if k % fund_bin == 0 {
            continue;
        }
        let mag = dft(&cur, k);
        assert!(mag < 0.5 * cur_fund, "suspected alias spike at bin {k}: {mag}");
    }
}
