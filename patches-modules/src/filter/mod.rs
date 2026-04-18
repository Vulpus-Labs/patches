//! Mono resonant biquad filters (lowpass, highpass, bandpass) and the shared
//! coefficient helpers they use. Each variant lives in its own submodule;
//! this file retains the RBJ cookbook coefficient functions that are also
//! consumed by `poly_filter` via `super::`.

use std::f32::consts::{FRAC_1_SQRT_2, TAU};

mod bandpass;
mod highpass;
mod lowpass;

pub use bandpass::ResonantBandpass;
pub use highpass::ResonantHighpass;
pub use lowpass::ResonantLowpass;

/// Maps normalised resonance [0, 1] to filter Q.
///
/// At 0.0 the Q equals the Butterworth value (≈ 0.707), giving a maximally
/// flat pass-band with no resonance peak. At 1.0 the Q is 10.0, producing
/// strong, audible resonance without self-oscillation.
#[inline]
fn resonance_to_q(resonance: f32) -> f32 {
    // 0.0 → Q = 1/√2 ≈ 0.707 (Butterworth), 1.0 → Q = 10.0
    FRAC_1_SQRT_2 + (10.0 - FRAC_1_SQRT_2) * resonance
}

/// Compute normalised biquad lowpass coefficients (a0 = 1).
///
/// Uses the Audio EQ Cookbook (RBJ) design equations. `cutoff_hz` is clamped
/// to [1, sample_rate × 0.499] to prevent instability near DC or Nyquist.
///
/// Returns `(b0, b1, b2, a1, a2)` ready for Transposed Direct Form II.
#[inline]
pub(crate) fn compute_biquad_lowpass(cutoff_hz: f32, resonance: f32, sample_rate: f32) -> (f32, f32, f32, f32, f32) {
    let q = resonance_to_q(resonance);
    let f = cutoff_hz.clamp(1.0, sample_rate * 0.499);
    let w0 = TAU * f / sample_rate;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = sin_w0 / (2.0 * q);
    let inv_a0 = 1.0 / (1.0 + alpha);
    let b0 = (1.0 - cos_w0) * 0.5 * inv_a0;
    let b1 = (1.0 - cos_w0) * inv_a0;
    let b2 = b0;
    let a1 = -2.0 * cos_w0 * inv_a0;
    let a2 = (1.0 - alpha) * inv_a0;
    (b0, b1, b2, a1, a2)
}

/// Compute normalised biquad highpass coefficients (a0 = 1).
///
/// Uses the Audio EQ Cookbook (RBJ) design equations. `cutoff_hz` is clamped
/// to [1, sample_rate × 0.499]. Returns `(b0, b1, b2, a1, a2)` for TDFII.
#[inline]
pub(crate) fn compute_biquad_highpass(cutoff_hz: f32, resonance: f32, sample_rate: f32) -> (f32, f32, f32, f32, f32) {
    let q = resonance_to_q(resonance);
    let f = cutoff_hz.clamp(1.0, sample_rate * 0.499);
    let w0 = TAU * f / sample_rate;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = sin_w0 / (2.0 * q);
    let inv_a0 = 1.0 / (1.0 + alpha);
    let b0 = (1.0 + cos_w0) * 0.5 * inv_a0;
    let b1 = -(1.0 + cos_w0) * inv_a0;
    let b2 = b0;
    let a1 = -2.0 * cos_w0 * inv_a0;
    let a2 = (1.0 - alpha) * inv_a0;
    (b0, b1, b2, a1, a2)
}

/// Compute normalised biquad bandpass coefficients (constant 0 dB peak gain, a0 = 1).
///
/// Uses the Audio EQ Cookbook (RBJ) design equations. `center_hz` is clamped
/// to [1, sample_rate × 0.499]. Returns `(b0, b1, b2, a1, a2)` for TDFII.
#[inline]
pub(crate) fn compute_biquad_bandpass(center_hz: f32, bandwidth_q: f32, sample_rate: f32) -> (f32, f32, f32, f32, f32) {
    let f = center_hz.clamp(1.0, sample_rate * 0.499);
    let w0 = TAU * f / sample_rate;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = sin_w0 / (2.0 * bandwidth_q);
    let inv_a0 = 1.0 / (1.0 + alpha);
    let b0 = alpha * inv_a0;
    let b1 = 0.0;
    let b2 = -alpha * inv_a0;
    let a1 = -2.0 * cos_w0 * inv_a0;
    let a2 = (1.0 - alpha) * inv_a0;
    (b0, b1, b2, a1, a2)
}

#[cfg(test)]
mod tests;
