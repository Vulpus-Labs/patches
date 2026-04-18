use super::*;
use super::analog_prototypes::{butterworth_bp, butterworth_hp, butterworth_lp};

// ─────────────────────────────────────────────────────────────────────────
// New DSP tests (T-0207 acceptance criteria T1–T7)
// ─────────────────────────────────────────────────────────────────────────

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
