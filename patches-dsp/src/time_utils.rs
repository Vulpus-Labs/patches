//! Time-domain utility functions shared across DSP kernels.
//!
//! These functions convert between millisecond-based parameters and
//! sample-domain values used by envelope followers, limiters, and
//! other time-dependent processors.

/// Convert milliseconds to a whole number of samples.
///
/// Clamps negative and NaN inputs to zero.
#[inline]
pub fn ms_to_samples(ms: f32, sample_rate: f32) -> usize {
    let raw = ms * 0.001 * sample_rate;
    if raw > 0.0 { raw.round() as usize } else { 0 }
}

/// Compute a one-pole smoothing coefficient for a given time constant.
///
/// Uses the standard formula: `coeff = 1 - exp(-1 / (time_ms * 0.001 * sample_rate))`.
/// Returns 1.0 (instant) for zero or negative time values.
#[inline]
pub fn compute_time_coeff(time_ms: f32, sample_rate: f32) -> f32 {
    if time_ms <= 0.0 {
        return 1.0;
    }
    1.0 - (-1.0_f32 / (time_ms * 0.001 * sample_rate)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ms_to_samples_basic() {
        assert_eq!(ms_to_samples(1.0, 44_100.0), 44);
        assert_eq!(ms_to_samples(10.0, 48_000.0), 480);
        assert_eq!(ms_to_samples(0.0, 44_100.0), 0);
    }

    #[test]
    fn ms_to_samples_negative_clamps_to_zero() {
        assert_eq!(ms_to_samples(-5.0, 44_100.0), 0);
    }

    #[test]
    fn compute_time_coeff_zero_is_instant() {
        assert_eq!(compute_time_coeff(0.0, 44_100.0), 1.0);
        assert_eq!(compute_time_coeff(-1.0, 44_100.0), 1.0);
    }

    #[test]
    fn compute_time_coeff_longer_time_is_smaller() {
        let short = compute_time_coeff(1.0, 44_100.0);
        let long = compute_time_coeff(100.0, 44_100.0);
        assert!(short > long, "shorter time should give larger coefficient");
    }

    #[test]
    fn compute_time_coeff_in_valid_range() {
        let c = compute_time_coeff(10.0, 48_000.0);
        assert!(c > 0.0 && c < 1.0, "coefficient should be in (0, 1), got {c}");
    }
}
