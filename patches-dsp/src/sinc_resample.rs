//! Windowed sinc resampler for offline sample-rate conversion.
//!
//! Uses a Kaiser-windowed sinc interpolation kernel. This is not intended for
//! real-time use — it processes an entire buffer at once and allocates the
//! output. Suitable for resampling impulse responses at load time.

use std::f64::consts::PI;

/// Number of zero crossings on each side of the sinc centre.
const SINC_HALF_LEN: usize = 16;

/// Kaiser window beta — controls sidelobe suppression.
/// β ≈ 6.5 gives ~-70 dB sidelobes, good enough for IR resampling.
const KAISER_BETA: f64 = 6.5;

/// Approximate I₀(x) (modified Bessel function of the first kind, order 0)
/// using the standard power-series truncated at 20 terms.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half_x = x * 0.5;
    for k in 1..=20 {
        term *= (half_x / k as f64) * (half_x / k as f64);
        sum += term;
    }
    sum
}

/// Kaiser window value at position `n` within a window of length `len`.
#[cfg(test)]
fn kaiser(n: usize, len: usize, beta: f64) -> f64 {
    let alpha = (len as f64 - 1.0) * 0.5;
    let ratio = (n as f64 - alpha) / alpha;
    let arg = beta * (1.0 - ratio * ratio).max(0.0).sqrt();
    bessel_i0(arg) / bessel_i0(beta)
}

/// Resample `input` from `from_rate` to `to_rate` using windowed sinc interpolation.
///
/// Returns a newly-allocated buffer of the resampled signal.
pub fn resample(input: &[f32], from_rate: f64, to_rate: f64) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }

    let ratio = to_rate / from_rate;
    let out_len = ((input.len() as f64) * ratio).ceil() as usize;
    let mut output = Vec::with_capacity(out_len);

    // When downsampling, widen the sinc kernel to act as a low-pass filter
    // at the Nyquist of the lower rate, preventing aliasing.
    let (filter_scale, sinc_scale) = if ratio < 1.0 {
        (ratio, ratio)
    } else {
        (1.0, 1.0)
    };

    let half = SINC_HALF_LEN as f64;
    let kernel_half_len = (half / filter_scale).ceil() as isize;
    let window_len = (2 * kernel_half_len + 1) as usize;

    let inv_bessel = 1.0 / bessel_i0(KAISER_BETA);

    for out_idx in 0..out_len {
        let centre = out_idx as f64 / ratio;
        let centre_i = centre.floor() as isize;

        let mut sum = 0.0f64;
        let mut weight_sum = 0.0f64;

        let start = (centre_i - kernel_half_len).max(0);
        let end = (centre_i + kernel_half_len).min(input.len() as isize - 1);

        for i in start..=end {
            let delta = (i as f64 - centre) * sinc_scale;

            // sinc
            let sinc_val = if delta.abs() < 1e-12 {
                1.0
            } else {
                (PI * delta).sin() / (PI * delta)
            };

            // Kaiser window position
            let win_pos = (i - centre_i + kernel_half_len) as usize;
            let alpha = (window_len as f64 - 1.0) * 0.5;
            let win_ratio = (win_pos as f64 - alpha) / alpha;
            let win_arg = KAISER_BETA * (1.0 - win_ratio * win_ratio).max(0.0).sqrt();
            let win_val = bessel_i0(win_arg) * inv_bessel;

            let w = sinc_val * win_val;
            sum += input[i as usize] as f64 * w;
            weight_sum += w;
        }

        // Normalise to preserve gain.
        let sample = if weight_sum.abs() > 1e-12 {
            sum / weight_sum
        } else {
            0.0
        };
        output.push(sample as f32);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_resample() {
        // Resampling at 1:1 ratio should return approximately the same signal.
        let input: Vec<f32> = (0..100).map(|i| (i as f32 * 0.1).sin()).collect();
        let output = resample(&input, 44100.0, 44100.0);
        assert_eq!(output.len(), input.len());
        // Measured ~0 error on aarch64 macOS debug (2026-04-02). Tightened from 0.01.
        for (a, b) in input.iter().zip(output.iter()) {
            assert!((a - b).abs() < 1e-4, "mismatch: {a} vs {b}");
        }
    }

    #[test]
    fn upsample_length() {
        let input = vec![0.0f32; 100];
        let output = resample(&input, 22050.0, 44100.0);
        assert_eq!(output.len(), 200);
    }

    #[test]
    fn downsample_length() {
        let input = vec![0.0f32; 100];
        let output = resample(&input, 44100.0, 22050.0);
        assert_eq!(output.len(), 50);
    }

    #[test]
    fn dc_signal_preserved() {
        // A DC signal should resample to another DC signal of the same level.
        let input = vec![0.75f32; 200];
        let output = resample(&input, 48000.0, 44100.0);
        // Skip edges where the kernel extends past input bounds.
        for &s in &output[20..output.len() - 20] {
            assert!((s - 0.75).abs() < 0.01, "DC not preserved: {s}");
        }
    }

    #[test]
    fn empty_input() {
        let output = resample(&[], 44100.0, 48000.0);
        assert!(output.is_empty());
    }

    #[test]
    fn bessel_i0_at_zero() {
        assert!((bessel_i0(0.0) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn kaiser_window_symmetric() {
        let len = 33;
        for i in 0..len / 2 {
            let left = kaiser(i, len, KAISER_BETA);
            let right = kaiser(len - 1 - i, len, KAISER_BETA);
            assert!(
                (left - right).abs() < 1e-10,
                "asymmetric at {i}: {left} vs {right}"
            );
        }
    }
}
