//! Biquad tests. Split by category from the original 738-line `tests.rs`
//! per ticket 0535. Shared fixtures and helpers live here; category-
//! specific tests live in sibling submodules.

#![allow(unused_imports)]

pub(super) use super::*;
pub(super) use crate::test_support::assert_reset_deterministic;

// ── Constants replicating patches_core values (avoids a dev-dependency) ──
pub(super) const BASE_PERIODIC_UPDATE_INTERVAL: u32 = 32;
pub(super) const COEFF_UPDATE_INTERVAL: u32 = BASE_PERIODIC_UPDATE_INTERVAL;

/// Evaluate the biquad transfer function H(z) at normalised frequency `f`
/// (cycles/sample), using f64 arithmetic for reference.
pub(super) fn h_magnitude(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32, f: f64) -> f64 {
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

/// Drive with a sinusoid, measure steady-state amplitude, compare to
/// theoretical |H(e^{jw})| within ±0.5 dB.
pub(super) fn check_frequency_response(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32, test_f: f64) {
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

/// Feed DC (constant 1.0) for a long time; measure the output level.
pub(super) fn dc_gain(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> f64 {
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
pub(super) fn nyquist_gain(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> f64 {
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

mod analog_prototypes;
mod frequency_response;
mod poly;
