use super::*;
use crate::test_support::assert_reset_deterministic;

// ── Constants replicating patches_core values (avoids a dev-dependency) ──
const BASE_PERIODIC_UPDATE_INTERVAL: u32 = 32;
const COEFF_UPDATE_INTERVAL: u32 = BASE_PERIODIC_UPDATE_INTERVAL;

// ─────────────────────────────────────────────────────────────────────────
// Migrated PolyBiquad tests (from patches-modules/src/common/poly_biquad.rs)
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn set_static_fans_out_to_all_voices() {
    let mut pb = PolyBiquad::new_static(1.0, 2.0, 3.0, 4.0, 5.0);
    pb.set_static(0.1, 0.2, 0.3, 0.4, 0.5);
    for i in 0..16 {
        assert_eq!(pb.b0[i], 0.1);
        assert_eq!(pb.b1[i], 0.2);
        assert_eq!(pb.b2[i], 0.3);
        assert_eq!(pb.a1[i], 0.4);
        assert_eq!(pb.a2[i], 0.5);
        assert_eq!(pb.db0[i], 0.0);
        assert_eq!(pb.db1[i], 0.0);
        assert_eq!(pb.db2[i], 0.0);
        assert_eq!(pb.da1[i], 0.0);
        assert_eq!(pb.da2[i], 0.0);
    }
    assert!(!pb.has_cv);
}

#[test]
fn begin_ramp_voice_snaps_then_ramps() {
    let mut pb = PolyBiquad::new_static(0.0, 0.0, 0.0, 0.0, 0.0);
    let new_b0t = 1.0_f32;
    let recip = 1.0 / BASE_PERIODIC_UPDATE_INTERVAL as f32;
    pb.begin_ramp_voice(0, new_b0t, 0.0, 0.0, 0.0, 0.0, recip);

    let expected_delta = (new_b0t - 0.0) * recip;
    let approx_eq = |a: f32, b: f32| (a - b).abs() < 1e-7;

    assert!(
        approx_eq(pb.db0[0], expected_delta),
        "db0[0] should be {expected_delta}, got {}",
        pb.db0[0]
    );
    // Other voices must be untouched.
    for i in 1..16 {
        assert_eq!(pb.db0[i], 0.0);
        assert_eq!(pb.b0[i], 0.0);
    }
    assert!(pb.has_cv);
}

#[test]
fn tick_all_advances_deltas() {
    let mut pb = PolyBiquad::new_static(0.0, 0.0, 0.0, 0.0, 0.0);
    let recip = 1.0 / BASE_PERIODIC_UPDATE_INTERVAL as f32;
    pb.begin_ramp_voice(0, 1.0, 0.0, 0.0, 0.0, 0.0, recip);
    let delta = pb.db0[0];
    let b0_before = pb.b0[0];
    pb.tick_all(&[0.0f32; 16], false, true);
    let approx_eq = |a: f32, b: f32| (a - b).abs() < 1e-7;
    assert!(
        approx_eq(pb.b0[0], b0_before + delta),
        "b0[0] should have advanced by one delta step"
    );
}

#[test]
fn voices_are_independent() {
    let mut pb = PolyBiquad::new_static(0.0, 0.0, 0.0, 0.0, 0.0);
    let recip = 1.0 / BASE_PERIODIC_UPDATE_INTERVAL as f32;
    // Give voice 0 a high-gain b0 target, voice 1 stays zero.
    pb.begin_ramp_voice(0, 1.0, 0.0, 0.0, 0.0, 0.0, recip);
    pb.begin_ramp_voice(1, 0.0, 0.0, 0.0, 0.0, 0.0, recip);
    // Drive voice 0 to its target.
    for _ in 0..COEFF_UPDATE_INTERVAL {
        pb.tick_all(&[0.0f32; 16], false, true);
    }
    // Unit impulse.
    let x = [1.0f32; 16];
    let y = pb.tick_all(&x, false, true);
    assert!(
        y[0] > y[1],
        "voice 0 (b0→1) should produce a larger output than voice 1 (b0=0); y[0]={}, y[1]={}",
        y[0],
        y[1]
    );
}

// ─────────────────────────────────────────────────────────────────────────
// PolyBiquad spectral and stability coverage (T-0257)
// ─────────────────────────────────────────────────────────────────────────

/// All 16 voices of PolyBiquad match MonoBiquad within 1e-6 when driven
/// with identical coefficients and input.
#[test]
fn poly_snr_matches_mono() {
    let fc = 0.1_f64;
    let w0 = 2.0 * std::f64::consts::PI * fc;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let q = std::f64::consts::SQRT_2 / 2.0;
    let alpha = sin_w0 / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0 = ((1.0 - cos_w0) / 2.0 / a0) as f32;
    let b1 = ((1.0 - cos_w0) / a0) as f32;
    let b2 = ((1.0 - cos_w0) / 2.0 / a0) as f32;
    let a1 = (-2.0 * cos_w0 / a0) as f32;
    let a2 = ((1.0 - alpha) / a0) as f32;

    let mut mono = MonoBiquad::new(b0, b1, b2, a1, a2);
    let mut poly = PolyBiquad::new_static(b0, b1, b2, a1, a2);

    for i in 0..1024_usize {
        let x = ((i as f32) * 0.07).sin();
        let y_mono = mono.tick(x, false);
        let y_poly = poly.tick_all(&[x; 16], false, false);
        for (v, &yv) in y_poly.iter().enumerate() {
            assert!(
                (yv - y_mono).abs() < 1e-6,
                "voice {v} sample {i}: poly={yv}, mono={y_mono}"
            );
        }
    }
}

/// All 16 voices remain bounded under high-Q noise input.
#[test]
fn poly_stability_high_resonance() {
    let fc = 0.1_f64;
    let q = 10.0_f64;
    let w0 = 2.0 * std::f64::consts::PI * fc;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0 = ((1.0 - cos_w0) / 2.0 / a0) as f32;
    let b1 = ((1.0 - cos_w0) / a0) as f32;
    let b2 = ((1.0 - cos_w0) / 2.0 / a0) as f32;
    let a1 = (-2.0 * cos_w0 / a0) as f32;
    let a2 = ((1.0 - alpha) / a0) as f32;

    let mut poly = PolyBiquad::new_static(b0, b1, b2, a1, a2);

    for k in 0..10_000_usize {
        let x = ((k as f32 * 0.12345).sin() + (k as f32 * 0.6789).cos()) * 0.5;
        let y = poly.tick_all(&[x; 16], false, false);
        for (v, &yv) in y.iter().enumerate() {
            assert!(
                yv.is_finite() && yv.abs() < 1000.0,
                "voice {v} sample {k}: instability y={yv}"
            );
        }
    }
}

/// FFT-based lowpass frequency response for one poly voice matches mono thresholds.
#[test]
fn poly_frequency_response_lowpass() {
    use crate::test_support::{assert_passband_flat, assert_stopband_below, magnitude_response_db};

    let fc = 0.1_f64;
    let w0 = 2.0 * std::f64::consts::PI * fc;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let q = std::f64::consts::SQRT_2 / 2.0;
    let alpha = sin_w0 / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0 = ((1.0 - cos_w0) / 2.0 / a0) as f32;
    let b1 = ((1.0 - cos_w0) / a0) as f32;
    let b2 = ((1.0 - cos_w0) / 2.0 / a0) as f32;
    let a1 = (-2.0 * cos_w0 / a0) as f32;
    let a2 = ((1.0 - alpha) / a0) as f32;

    let fft_size = 1024;
    let mut poly = PolyBiquad::new_static(b0, b1, b2, a1, a2);

    // Capture impulse response from voice 0
    let mut ir = vec![0.0f32; fft_size];
    for (k, sample) in ir.iter_mut().enumerate() {
        let x = if k == 0 { 1.0 } else { 0.0 };
        let y = poly.tick_all(&[x; 16], false, false);
        *sample = y[0];
    }

    let db = magnitude_response_db(&ir, fft_size);
    // Same thresholds as mono t0240_lowpass test
    assert_passband_flat!(db, 1..30, 1.0);
    assert_stopband_below!(db, 300..512, -20.0);
}

// ─────────────────────────────────────────────────────────────────────────
// New DSP tests (T-0207 acceptance criteria T1–T7)
// ─────────────────────────────────────────────────────────────────────────

/// Compute second-order Butterworth lowpass coefficients.
///
/// `fc` is the normalised cut-off frequency in [0, 0.5) (cycles/sample).
/// Returns (b0, b1, b2, a1, a2) for a TDFII biquad.
fn butterworth_lp(fc: f64) -> (f32, f32, f32, f32, f32) {
    use std::f64::consts::PI;
    let w0 = 2.0 * PI * fc;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    // Q = 1/sqrt(2) for Butterworth
    let q = std::f64::consts::SQRT_2 / 2.0; // 1/sqrt(2)
    let alpha = sin_w0 / (2.0 * q);

    let b0 = (1.0 - cos_w0) / 2.0;
    let b1 = 1.0 - cos_w0;
    let b2 = (1.0 - cos_w0) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;

    (
        (b0 / a0) as f32,
        (b1 / a0) as f32,
        (b2 / a0) as f32,
        (a1 / a0) as f32,
        (a2 / a0) as f32,
    )
}

/// Second-order highpass Butterworth coefficients.
fn butterworth_hp(fc: f64) -> (f32, f32, f32, f32, f32) {
    use std::f64::consts::PI;
    let w0 = 2.0 * PI * fc;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let q = std::f64::consts::SQRT_2 / 2.0;
    let alpha = sin_w0 / (2.0 * q);

    let b0 = (1.0 + cos_w0) / 2.0;
    let b1 = -(1.0 + cos_w0);
    let b2 = (1.0 + cos_w0) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;

    (
        (b0 / a0) as f32,
        (b1 / a0) as f32,
        (b2 / a0) as f32,
        (a1 / a0) as f32,
        (a2 / a0) as f32,
    )
}

/// Second-order bandpass (constant-skirt, peak gain = Q) coefficients.
fn butterworth_bp(fc: f64, q: f64) -> (f32, f32, f32, f32, f32) {
    use std::f64::consts::PI;
    let w0 = 2.0 * PI * fc;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);

    let b0 = sin_w0 / 2.0;
    let b1 = 0.0;
    let b2 = -sin_w0 / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha;

    (
        (b0 / a0) as f32,
        (b1 / a0) as f32,
        (b2 / a0) as f32,
        (a1 / a0) as f32,
        (a2 / a0) as f32,
    )
}

/// Evaluate the biquad transfer function H(z) at normalised frequency `f`
/// (cycles/sample), using f64 arithmetic for reference.
fn h_magnitude(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32, f: f64) -> f64 {
    use std::f64::consts::PI;
    let w = 2.0 * PI * f;
    // z = e^{jw}; H(z) = (b0 + b1*z^{-1} + b2*z^{-2}) / (1 + a1*z^{-1} + a2*z^{-2})
    let b0 = b0 as f64;
    let b1 = b1 as f64;
    let b2 = b2 as f64;
    let a1 = a1 as f64;
    let a2 = a2 as f64;
    let re_num = b0 + b1 * w.cos() + b2 * (2.0 * w).cos();
    let im_num = -(b1 * w.sin() + b2 * (2.0 * w).sin());
    let re_den = 1.0 + a1 * w.cos() + a2 * (2.0 * w).cos();
    let im_den = -(a1 * w.sin() + a2 * (2.0 * w).sin());
    ((re_num * re_num + im_num * im_num) / (re_den * re_den + im_den * im_den)).sqrt()
}

// ── T1: Impulse response ──────────────────────────────────────────────────

/// Feed a unit impulse through a Butterworth lowpass at Fc = 0.1·fs and
/// compare the first N output samples against the reference computed in f64.
#[test]
fn t1_impulse_response_butterworth_lp() {
    let fc = 0.1_f64;
    let (b0, b1, b2, a1, a2) = butterworth_lp(fc);

    // Reference: compute impulse response in f64.
    let n = 32_usize;
    let mut ref_s1 = 0.0_f64;
    let mut ref_s2 = 0.0_f64;
    let b0d = b0 as f64;
    let b1d = b1 as f64;
    let b2d = b2 as f64;
    let a1d = a1 as f64;
    let a2d = a2 as f64;
    let mut reference = vec![0.0_f64; n];
    for (k, r) in reference.iter_mut().enumerate() {
        let x = if k == 0 { 1.0_f64 } else { 0.0_f64 };
        let y = b0d * x + ref_s1;
        ref_s1 = b1d * x - a1d * y + ref_s2;
        ref_s2 = b2d * x - a2d * y;
        *r = y;
    }

    // f32 filter.
    // Tolerance: f32 has ~7 significant digits; coefficients are cast from
    // f64 to f32, so each sample accumulates ~1e-7 relative error.
    let mut filt = MonoBiquad::new(b0, b1, b2, a1, a2);
    for (k, &ref_y) in reference.iter().enumerate() {
        let x = if k == 0 { 1.0_f32 } else { 0.0_f32 };
        let y = filt.tick(x, false);
        let err = (y as f64 - ref_y).abs();
        assert!(
            err < 1e-6,
            "sample {k}: got {y}, ref {ref_y:.12}, err {err:.3e}"
        );
    }
}

// ── T2: Frequency response ────────────────────────────────────────────────

/// Drive with a sinusoid, measure steady-state amplitude, compare to
/// theoretical |H(e^{jw})| within ±0.5 dB.
fn check_frequency_response(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32, test_f: f64) {
    use std::f64::consts::PI;
    let mut filt = MonoBiquad::new(b0, b1, b2, a1, a2);

    // Warm-up: 4096 samples
    for k in 0..4096_usize {
        let x = (2.0 * PI * test_f * k as f64).sin() as f32;
        filt.tick(x, false);
    }

    // Measure RMS amplitude over the next 1024 samples.
    let measure_n = 1024_usize;
    let mut sum_sq = 0.0_f64;
    for k in 4096..4096 + measure_n {
        let x = (2.0 * PI * test_f * k as f64).sin() as f32;
        let y = filt.tick(x, false) as f64;
        sum_sq += y * y;
    }
    let rms = (sum_sq / measure_n as f64).sqrt();
    // Input RMS is 1/sqrt(2).
    let gain = rms * std::f64::consts::SQRT_2;

    let theory = h_magnitude(b0, b1, b2, a1, a2, test_f);
    let db_diff = 20.0 * (gain / theory.max(1e-12)).log10();
    assert!(
        db_diff.abs() < 0.5,
        "f={test_f}: measured gain={gain:.6}, theory={theory:.6}, diff={db_diff:.3} dB"
    );
}

#[test]
fn t2_lowpass_frequency_response() {
    let (b0, b1, b2, a1, a2) = butterworth_lp(0.1);
    // Passband: well below cutoff
    check_frequency_response(b0, b1, b2, a1, a2, 0.01);
    // At cutoff (-3 dB point)
    check_frequency_response(b0, b1, b2, a1, a2, 0.1);
    // Stopband: above cutoff
    check_frequency_response(b0, b1, b2, a1, a2, 0.3);
}

#[test]
fn t2_highpass_frequency_response() {
    let (b0, b1, b2, a1, a2) = butterworth_hp(0.1);
    // Passband: well above cutoff
    check_frequency_response(b0, b1, b2, a1, a2, 0.4);
    // At cutoff
    check_frequency_response(b0, b1, b2, a1, a2, 0.1);
    // Stopband: below cutoff
    check_frequency_response(b0, b1, b2, a1, a2, 0.02);
}

#[test]
fn t2_bandpass_frequency_response() {
    let fc = 0.2_f64;
    let q = 2.0_f64;
    let (b0, b1, b2, a1, a2) = butterworth_bp(fc, q);
    // At centre frequency, gain should be near peak
    check_frequency_response(b0, b1, b2, a1, a2, fc);
    // Well off centre: in the stopband
    check_frequency_response(b0, b1, b2, a1, a2, 0.02);
}

// ── T3: DC and Nyquist ────────────────────────────────────────────────────

/// Feed DC (constant 1.0) for a long time; measure the output level.
fn dc_gain(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> f64 {
    let mut filt = MonoBiquad::new(b0, b1, b2, a1, a2);
    for _ in 0..4096 {
        filt.tick(1.0, false);
    }
    let mut sum = 0.0_f64;
    for _ in 0..1024 {
        sum += filt.tick(1.0, false) as f64;
    }
    sum / 1024.0
}

/// Feed Nyquist (alternating ±1) for a long time; measure the output level.
///
/// The input is a ±1 square wave at Nyquist (every sample alternates sign).
/// Its RMS amplitude is 1.0.  The output RMS divided by the input RMS gives
/// the gain — no `sqrt(2)` correction is needed because the input is already
/// at its peak, not a sinusoid.
fn nyquist_gain(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> f64 {
    let mut filt = MonoBiquad::new(b0, b1, b2, a1, a2);
    for k in 0..4096_usize {
        filt.tick(if k % 2 == 0 { 1.0 } else { -1.0 }, false);
    }
    let mut sum_sq = 0.0_f64;
    for k in 0..1024_usize {
        let x = if k % 2 == 0 { 1.0_f32 } else { -1.0_f32 };
        let y = filt.tick(x, false) as f64;
        sum_sq += y * y;
    }
    // RMS output / RMS input (= 1.0 for ±1 square wave).
    (sum_sq / 1024.0_f64).sqrt()
}

#[test]
fn t3_lowpass_dc_unity() {
    let (b0, b1, b2, a1, a2) = butterworth_lp(0.1);
    let g = dc_gain(b0, b1, b2, a1, a2);
    assert!(
        (g - 1.0).abs() < 0.001,
        "lowpass DC gain should be ≈1.0, got {g}"
    );
}

#[test]
fn t3_highpass_attenuates_dc() {
    let (b0, b1, b2, a1, a2) = butterworth_hp(0.1);
    let g = dc_gain(b0, b1, b2, a1, a2);
    assert!(g < 0.01, "highpass DC gain should be ≈0, got {g}");
}

#[test]
fn t3_bandpass_attenuates_dc() {
    let (b0, b1, b2, a1, a2) = butterworth_bp(0.2, 2.0);
    let g = dc_gain(b0, b1, b2, a1, a2);
    assert!(g < 0.01, "bandpass DC gain should be ≈0, got {g}");
}

#[test]
fn t3_highpass_passes_nyquist() {
    let (b0, b1, b2, a1, a2) = butterworth_hp(0.1);
    let g = nyquist_gain(b0, b1, b2, a1, a2);
    // At Nyquist, 2nd-order Butterworth HP has gain ≈ 1.0.
    assert!(
        (g - 1.0).abs() < 0.01,
        "highpass Nyquist gain should be ≈1.0, got {g}"
    );
}

#[test]
fn t3_lowpass_attenuates_nyquist() {
    let (b0, b1, b2, a1, a2) = butterworth_lp(0.1);
    let g = nyquist_gain(b0, b1, b2, a1, a2);
    assert!(g < 0.01, "lowpass Nyquist gain should be ≈0, got {g}");
}

// ── T4: Stability (high-resonance) ────────────────────────────────────────

#[test]
fn t4_high_resonance_stability() {
    // Near-resonant lowpass: Q ≈ 10
    use std::f64::consts::PI;
    let fc = 0.1_f64;
    let q = 10.0_f64;
    let w0 = 2.0 * PI * fc;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);
    let b0f = ((1.0 - cos_w0) / 2.0 / (1.0 + alpha)) as f32;
    let b1f = ((1.0 - cos_w0) / (1.0 + alpha)) as f32;
    let b2f = ((1.0 - cos_w0) / 2.0 / (1.0 + alpha)) as f32;
    let a1f = (-2.0 * cos_w0 / (1.0 + alpha)) as f32;
    let a2f = ((1.0 - alpha) / (1.0 + alpha)) as f32;

    let mut filt = MonoBiquad::new(b0f, b1f, b2f, a1f, a2f);

    for k in 0..10_000_usize {
        // White-ish noise-like input (bounded ±1).
        let x = ((k as f32 * 0.12345).sin() + (k as f32 * 0.6789).cos()) * 0.5;
        let y = filt.tick(x, false);
        assert!(
            y.is_finite() && y.abs() < 1000.0,
            "instability at sample {k}: y = {y}"
        );
    }
}

// ── T6: SNR and precision ─────────────────────────────────────────────────

/// T6 — SNR and precision
///
/// Run a Butterworth lowpass (fc = 1000 Hz, sr = 48000 Hz) on 10,000 samples
/// of a 1 kHz sinusoid in both f32 (MonoBiquad) and an inline f64 reference.
/// Compute RMS error and assert SNR ≥ 60 dB.
#[test]
fn t6_snr_butterworth_lp_vs_f64_reference() {
    use std::f64::consts::PI;

    const SR: f64 = 48_000.0;
    const FC: f64 = 1_000.0;
    const DRIVE_HZ: f64 = 1_000.0;
    const N: usize = 10_000;

    // Compute normalised Butterworth LP coefficients in f64.
    let w0 = 2.0 * PI * FC / SR;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let q = std::f64::consts::SQRT_2 / 2.0; // 1/sqrt(2) Butterworth
    let alpha = sin_w0 / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0d = (1.0 - cos_w0) / 2.0 / a0;
    let b1d = (1.0 - cos_w0) / a0;
    let b2d = (1.0 - cos_w0) / 2.0 / a0;
    let a1d = -2.0 * cos_w0 / a0;
    let a2d = (1.0 - alpha) / a0;

    // f32 filter.
    let mut filt = MonoBiquad::new(b0d as f32, b1d as f32, b2d as f32, a1d as f32, a2d as f32);

    // f64 reference state (TDFII).
    let mut ref_s1 = 0.0_f64;
    let mut ref_s2 = 0.0_f64;

    let mut sum_sq_signal = 0.0_f64;
    let mut sum_sq_error = 0.0_f64;

    for k in 0..N {
        let x64 = (2.0 * PI * DRIVE_HZ / SR * k as f64).sin();
        let x32 = x64 as f32;

        // f64 reference TDFII recurrence.
        let ref_y = b0d * x64 + ref_s1;
        ref_s1 = b1d * x64 - a1d * ref_y + ref_s2;
        ref_s2 = b2d * x64 - a2d * ref_y;

        // f32 filter.
        let y32 = filt.tick(x32, false) as f64;

        sum_sq_signal += ref_y * ref_y;
        let err = y32 - ref_y;
        sum_sq_error += err * err;
    }

    let rms_signal = (sum_sq_signal / N as f64).sqrt();
    let rms_error = (sum_sq_error / N as f64).sqrt();
    let snr_db = 20.0 * (rms_signal / rms_error).log10();

    // Measured 122.7 dB on aarch64 macOS debug (2026-04-02). Tightened from 60 dB.
    assert!(
        snr_db >= 100.0,
        "SNR too low: {snr_db:.1} dB (expected ≥ 100 dB); rms_signal={rms_signal:.6}, rms_error={rms_error:.6}"
    );
}

// ── T7: Determinism ───────────────────────────────────────────────────────

#[test]
fn t7_determinism_after_reset() {
    let (b0, b1, b2, a1, a2) = butterworth_lp(0.1);
    let input: Vec<f32> = (0..256).map(|k| ((k as f32) * 0.07).sin()).collect();

    assert_reset_deterministic!(
        MonoBiquad::new(b0, b1, b2, a1, a2),
        &input,
        |f: &mut MonoBiquad, x: f32| f.tick(x, false),
        |f: &mut MonoBiquad| { f.reset_state(); f.set_static(b0, b1, b2, a1, a2); }
    );
}

// ── T-0240: Frequency response via magnitude_response_db ─────────────────

#[test]
fn t0240_lowpass_passband_flat_stopband_attenuated() {
    use crate::test_support::{assert_passband_flat, assert_stopband_below, magnitude_response_db};

    let fft_size = 1024;
    let (b0, b1, b2, a1, a2) = butterworth_lp(0.1); // cutoff at 0.1 * Nyquist
    let mut filt = MonoBiquad::new(b0, b1, b2, a1, a2);

    // Capture impulse response.
    let mut ir = vec![0.0f32; fft_size];
    for (k, sample) in ir.iter_mut().enumerate() {
        let x = if k == 0 { 1.0 } else { 0.0 };
        *sample = filt.tick(x, false);
    }

    let db = magnitude_response_db(&ir, fft_size);
    // fc=0.1 normalised → cutoff at bin 0.1 * N/2 = 51. Passband well below,
    // stopband well above. Second-order rolloff is -12 dB/oct.
    assert_passband_flat!(db, 1..30, 1.0);
    assert_stopband_below!(db, 300..512, -20.0);
}

#[test]
fn t0240_highpass_passband_flat_stopband_attenuated() {
    use crate::test_support::{assert_passband_flat, assert_stopband_below, magnitude_response_db};

    let fft_size = 1024;
    let (b0, b1, b2, a1, a2) = butterworth_hp(0.1);
    let mut filt = MonoBiquad::new(b0, b1, b2, a1, a2);

    let mut ir = vec![0.0f32; fft_size];
    for (k, sample) in ir.iter_mut().enumerate() {
        let x = if k == 0 { 1.0 } else { 0.0 };
        *sample = filt.tick(x, false);
    }

    let db = magnitude_response_db(&ir, fft_size);
    // Highpass: passband at high bins, stopband at low bins
    assert_passband_flat!(db, 200..500, 1.0);
    assert_stopband_below!(db, 1..20, -20.0);
}

#[test]
fn t0240_bandpass_peaks_at_centre() {
    use crate::test_support::{assert_peak_at_bin, magnitude_spectrum};
    use crate::fft::RealPackedFft;

    let fft_size = 1024;
    let fc = 0.2_f64; // centre frequency
    let q = 4.0_f64;
    let (b0, b1, b2, a1, a2) = butterworth_bp(fc, q);
    let mut filt = MonoBiquad::new(b0, b1, b2, a1, a2);

    let mut ir = vec![0.0f32; fft_size];
    for (k, sample) in ir.iter_mut().enumerate() {
        let x = if k == 0 { 1.0 } else { 0.0 };
        *sample = filt.tick(x, false);
    }

    let fft = RealPackedFft::new(fft_size);
    let mut buf = vec![0.0f32; fft_size];
    buf.copy_from_slice(&ir);
    fft.forward(&mut buf);
    let spectrum = magnitude_spectrum(&buf, fft_size);

    // fc=0.2 normalised (cycles/sample) → bin = fc * N = 0.2 * 1024 ≈ 205.
    let expected_bin = (fc * fft_size as f64).round() as usize;
    assert_peak_at_bin!(spectrum, expected_bin, 3);
}

// ── T-0240: Saturate flag ────────────────────────────────────────────────

#[test]
fn t0240_saturate_clips_output() {
    // With high resonance, the unsaturated filter can produce large peaks.
    // The saturate flag should tame those peaks via tanh soft-clipping.
    let (b0, b1, b2, a1, a2) = butterworth_lp(0.1);
    let mut filt_sat = MonoBiquad::new(b0, b1, b2, a1, a2);
    let mut filt_dry = MonoBiquad::new(b0, b1, b2, a1, a2);

    // Drive with a large impulse to excite resonance.
    let impulse = 10.0_f32;
    let n = 256;
    let mut peak_sat = 0.0_f32;
    let mut peak_dry = 0.0_f32;
    for k in 0..n {
        let x = if k == 0 { impulse } else { 0.0 };
        let y_sat = filt_sat.tick(x, true);
        let y_dry = filt_dry.tick(x, false);
        peak_sat = peak_sat.max(y_sat.abs());
        peak_dry = peak_dry.max(y_dry.abs());
    }

    // Saturated output should be lower than unsaturated for a large impulse.
    // The tanh feedback keeps the internal state bounded.
    assert!(
        peak_sat <= peak_dry,
        "saturated peak ({peak_sat}) should not exceed dry peak ({peak_dry})"
    );
}

// ── T-0240: Coefficient stability at extremes ────────────────────────────

#[test]
fn t0240_coefficient_stability_extreme_values() {
    use std::f64::consts::PI;

    // Test extreme cutoff and Q combinations.
    let cases: &[(f64, f64, &str)] = &[
        (0.001, 0.5, "very low cutoff"),
        (0.499, 0.5, "near Nyquist cutoff"),
        (0.1, 100.0, "very high Q"),
        (0.001, 100.0, "low cutoff + high Q"),
        (0.499, 100.0, "near Nyquist + high Q"),
    ];

    for &(fc, q, label) in cases {
        let w0 = 2.0 * PI * fc;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);
        let a0 = 1.0 + alpha;
        let b0 = ((1.0 - cos_w0) / 2.0 / a0) as f32;
        let b1 = ((1.0 - cos_w0) / a0) as f32;
        let b2 = ((1.0 - cos_w0) / 2.0 / a0) as f32;
        let a1 = (-2.0 * cos_w0 / a0) as f32;
        let a2 = ((1.0 - alpha) / a0) as f32;

        // Coefficients must be finite.
        assert!(b0.is_finite() && b1.is_finite() && b2.is_finite(), "{label}: non-finite b coefficients");
        assert!(a1.is_finite() && a2.is_finite(), "{label}: non-finite a coefficients");

        // Filter must not blow up over 1000 samples.
        let mut filt = MonoBiquad::new(b0, b1, b2, a1, a2);
        for k in 0..1000_usize {
            let x = ((k as f32 * 0.07).sin()) * 0.5;
            let y = filt.tick(x, false);
            assert!(
                y.is_finite(),
                "{label}: non-finite output at sample {k}: y = {y}"
            );
        }
    }
}
