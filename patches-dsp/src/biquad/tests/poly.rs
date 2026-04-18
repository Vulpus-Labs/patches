use super::*;

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
