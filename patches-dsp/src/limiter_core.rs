//! Shared gain-envelope state machine for lookahead peak limiters.
//!
//! Encapsulates peak tracking, target gain calculation, and smoothed gain
//! reduction. Used by both mono [`Limiter`](super::Limiter) and stereo
//! [`StereoLimiter`](super::StereoLimiter) module wrappers.

use crate::{
    compute_time_coeff, ms_to_samples, HalfbandInterpolator, PeakWindow,
};

/// Shared gain-envelope core for lookahead peak limiters.
///
/// Owns the peak window and smoothed gain state. Module wrappers push
/// oversampled magnitudes into the core and read back the current gain.
pub struct LimiterCore {
    peak_window: PeakWindow,
    current_gain: f32,
    lookahead_samples: usize,
    threshold_internal: f32,
    attack_coeff: f32,
    release_coeff: f32,
    sample_rate: f32,
    attack_ms: f32,
    release_ms: f32,
}

impl LimiterCore {
    /// Create a new `LimiterCore` with the given parameters and sample rate.
    ///
    /// `max_attack_ms` determines the maximum lookahead window size and must
    /// match the upper bound of the `attack_ms` parameter.
    pub fn new(
        sample_rate: f32,
        threshold: f32,
        attack_ms: f32,
        release_ms: f32,
        max_attack_ms: f32,
    ) -> Self {
        let attack_coeff = compute_time_coeff(attack_ms, sample_rate);
        let release_coeff = compute_time_coeff(release_ms, sample_rate);
        let lookahead_samples = ms_to_samples(attack_ms, sample_rate);
        let max_lookahead = ms_to_samples(max_attack_ms, sample_rate);

        let mut peak_window = PeakWindow::new(2 * (max_lookahead + 1));
        peak_window.set_window(2 * (lookahead_samples + 1));

        Self {
            peak_window,
            current_gain: 1.0,
            lookahead_samples,
            threshold_internal: threshold.max(0.0) * 0.98,
            attack_coeff,
            release_coeff,
            sample_rate,
            attack_ms,
            release_ms,
        }
    }

    /// Update the threshold parameter.
    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold_internal = threshold.max(0.0) * 0.98;
    }

    /// Update the attack time. Returns `true` if the value changed.
    pub fn set_attack_ms(&mut self, new_ms: f32, max_attack_ms: f32) -> bool {
        let new_ms = new_ms.clamp(0.1, max_attack_ms);
        if (new_ms - self.attack_ms).abs() > f32::EPSILON {
            self.attack_ms = new_ms;
            self.attack_coeff = compute_time_coeff(new_ms, self.sample_rate);
            self.lookahead_samples = ms_to_samples(new_ms, self.sample_rate);
            self.peak_window.set_window(2 * (self.lookahead_samples + 1));
            true
        } else {
            false
        }
    }

    /// Update the release time.
    pub fn set_release_ms(&mut self, new_ms: f32) {
        let new_ms = new_ms.max(1.0);
        if (new_ms - self.release_ms).abs() > f32::EPSILON {
            self.release_ms = new_ms;
            self.release_coeff = compute_time_coeff(new_ms, self.sample_rate);
        }
    }

    /// Push an oversampled magnitude into the peak window and update the gain.
    ///
    /// Call this twice per base-rate sample (once for each oversampled output).
    /// After both pushes, call [`current_gain`](Self::current_gain) to read the
    /// smoothed gain value.
    #[inline]
    pub fn push_magnitude(&mut self, magnitude: f32) {
        self.peak_window.push(magnitude);
    }

    /// Compute the smoothed gain after all magnitudes for this sample have been pushed.
    #[inline]
    pub fn update_gain(&mut self) {
        let peak = self.peak_window.peak();
        let target_gain = if peak > self.threshold_internal {
            (self.threshold_internal / peak).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let coeff = if target_gain < self.current_gain {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.current_gain += coeff * (target_gain - self.current_gain);
    }

    /// The current smoothed gain value.
    #[inline]
    pub fn current_gain(&self) -> f32 {
        self.current_gain
    }

    /// The current lookahead in samples.
    pub fn lookahead_samples(&self) -> usize {
        self.lookahead_samples
    }

    /// The total read offset for the dry delay line (lookahead + FIR group delay).
    pub fn read_offset(&self) -> usize {
        self.lookahead_samples + HalfbandInterpolator::GROUP_DELAY_BASE_RATE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn below_threshold_gain_is_unity() {
        let mut core = LimiterCore::new(SR, 0.9, 2.0, 100.0, 50.0);
        // Push quiet magnitudes for several samples
        for _ in 0..200 {
            core.push_magnitude(0.3);
            core.push_magnitude(0.3);
            core.update_gain();
        }
        assert!(
            (core.current_gain() - 1.0).abs() < 0.01,
            "gain should be near 1.0 below threshold, got {}",
            core.current_gain()
        );
    }

    #[test]
    fn above_threshold_reduces_gain() {
        let mut core = LimiterCore::new(SR, 0.9, 2.0, 100.0, 50.0);
        // Push loud magnitudes
        for _ in 0..2000 {
            core.push_magnitude(2.0);
            core.push_magnitude(2.0);
            core.update_gain();
        }
        assert!(
            core.current_gain() < 0.6,
            "gain should be reduced above threshold, got {}",
            core.current_gain()
        );
    }

    #[test]
    fn gain_recovery_does_not_produce_denormals() {
        let mut core = LimiterCore::new(SR, 0.9, 2.0, 100.0, 50.0);
        // Slam loud signal to drive gain down
        for _ in 0..2000 {
            core.push_magnitude(2.0);
            core.push_magnitude(2.0);
            core.update_gain();
        }
        assert!(core.current_gain() < 0.6);

        // Release into silence for 30 seconds
        for i in 0..(SR as usize * 30) {
            core.push_magnitude(0.0);
            core.push_magnitude(0.0);
            core.update_gain();
            let g = core.current_gain();
            // Gain should stay normal as it recovers toward 1.0
            assert!(
                g == 0.0 || g == 1.0 || g.is_normal(),
                "denormal gain at sample {i}: {g:e} (bits: {:#010x})",
                g.to_bits()
            );
        }
    }

    #[test]
    fn set_attack_updates_lookahead() {
        let mut core = LimiterCore::new(SR, 0.9, 2.0, 100.0, 50.0);
        let old_lookahead = core.lookahead_samples();
        core.set_attack_ms(10.0, 50.0);
        assert!(core.lookahead_samples() > old_lookahead);
    }
}
