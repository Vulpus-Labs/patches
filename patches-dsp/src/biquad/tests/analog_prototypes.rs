use super::*;

/// Compute second-order Butterworth lowpass coefficients.
///
/// `fc` is the normalised cut-off frequency in [0, 0.5) (cycles/sample).
/// Returns (b0, b1, b2, a1, a2) for a TDFII biquad.
pub(super) fn butterworth_lp(fc: f64) -> (f32, f32, f32, f32, f32) {
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
pub(super) fn butterworth_hp(fc: f64) -> (f32, f32, f32, f32, f32) {
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
pub(super) fn butterworth_bp(fc: f64, q: f64) -> (f32, f32, f32, f32, f32) {
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
