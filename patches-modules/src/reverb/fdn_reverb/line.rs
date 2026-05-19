//! Per-line delay geometry and high-shelf absorption coefficient design.

use std::f32::consts::TAU;

use super::matrix::LINES;

/// Base delay line lengths in milliseconds (before scale), shared across archetypes.
pub(super) const BASE_MS: [f32; LINES] = [29.7, 37.1, 41.1, 43.7, 53.3, 59.7, 67.1, 79.3];

/// Compute biquad high-shelf coefficients (TDFII, normalised by a0) for the
/// absorption filter on one delay line.
///
/// The shelf has DC gain `g_lf` and HF gain `g_hf`, with crossover at
/// `crossover_hz`.  Follows the Audio EQ Cookbook high-shelf design with S=1.
pub(super) fn absorption_coeffs(
    delay_ms:     f32,
    scale:        f32,
    rt60_lf:      f32,
    rt60_hf:      f32,
    crossover_hz: f32,
    sample_rate:  f32,
    sr_recip:     f32,
) -> (f32, f32, f32, f32, f32) {
    let delay_samp = delay_ms * scale * sample_rate * 0.001;
    let sr_safe    = sample_rate.max(1.0);
    let g_lf = 10.0_f32.powf(-3.0 * delay_samp / (rt60_lf * sr_safe).max(1.0));
    let g_hf = 10.0_f32.powf(-3.0 * delay_samp / (rt60_hf * sr_safe).max(1.0));

    // Shelf amplitude ratio: A = sqrt(g_hf / g_lf).
    // DC gain of the shelf filter = 1; HF gain = A^2 = g_hf/g_lf.
    // We then multiply b coefficients by g_lf so DC gain = g_lf.
    let a_ratio = (g_hf / g_lf.max(1e-30)).sqrt().clamp(0.001, 1000.0);
    let fc      = crossover_hz.clamp(20.0, sample_rate * 0.499);
    let w0      = TAU * fc * sr_recip;
    let cos_w0  = w0.cos();
    // S=1: alpha = sin(w0)/sqrt(2)
    let alpha   = w0.sin() * 0.707_106_77_f32;
    let sqrt_a  = a_ratio.sqrt();

    let b0 =  a_ratio * ((a_ratio+1.0) + (a_ratio-1.0)*cos_w0 + 2.0*sqrt_a*alpha);
    let b1 = -2.0*a_ratio * ((a_ratio-1.0) + (a_ratio+1.0)*cos_w0);
    let b2 =  a_ratio * ((a_ratio+1.0) + (a_ratio-1.0)*cos_w0 - 2.0*sqrt_a*alpha);
    let a0 =           (a_ratio+1.0) - (a_ratio-1.0)*cos_w0 + 2.0*sqrt_a*alpha;
    let a1 =  2.0 * ((a_ratio-1.0) - (a_ratio+1.0)*cos_w0);
    let a2 =           (a_ratio+1.0) - (a_ratio-1.0)*cos_w0 - 2.0*sqrt_a*alpha;

    // Normalise by a0, scale b by g_lf so DC gain = g_lf.
    let bs = g_lf / a0;
    (b0*bs, b1*bs, b2*bs, a1/a0, a2/a0)
}
