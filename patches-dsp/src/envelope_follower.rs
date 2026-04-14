//! Envelope follower with separate attack and release time constants.
//!
//! Tracks the amplitude envelope of an input signal using exponential
//! smoothing with independent attack (rising) and release (falling)
//! coefficients.
//!
//! ```text
//! if |input| > envelope:
//!     envelope += attack_coeff * (|input| - envelope)
//! else:
//!     envelope += release_coeff * (|input| - envelope)
//! ```
//!
//! Coefficients are derived from time in milliseconds using the standard
//! formula: `coeff = 1 - exp(-1 / (time_ms * 0.001 * sample_rate))`.

/// Per-sample envelope follower with independent attack and release.
#[derive(Clone)]
pub struct EnvelopeFollower {
    envelope: f32,
    attack_coeff: f32,
    release_coeff: f32,
}

impl EnvelopeFollower {
    pub fn new() -> Self {
        Self {
            envelope: 0.0,
            attack_coeff: 0.1,
            release_coeff: 0.01,
        }
    }

    /// Set the attack time in milliseconds.
    pub fn set_attack_ms(&mut self, ms: f32, sample_rate: f32) {
        self.attack_coeff = crate::compute_time_coeff(ms, sample_rate);
    }

    /// Set the release time in milliseconds.
    pub fn set_release_ms(&mut self, ms: f32, sample_rate: f32) {
        self.release_coeff = crate::compute_time_coeff(ms, sample_rate);
    }

    /// Process one sample, returning the current envelope value.
    #[inline]
    pub fn tick(&mut self, input: f32) -> f32 {
        let abs_input = input.abs();
        let coeff = if abs_input > self.envelope {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.envelope = crate::flush_denormal(self.envelope + coeff * (abs_input - self.envelope));
        self.envelope
    }

    /// Return the current envelope value without advancing state.
    #[inline]
    pub fn current(&self) -> f32 {
        self.envelope
    }

    /// Reset envelope state to zero.
    pub fn reset(&mut self) {
        self.envelope = 0.0;
    }
}

impl Default for EnvelopeFollower {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_within;

    const SR: f32 = 44_100.0;

    #[test]
    fn step_response_rises_to_target() {
        let mut ef = EnvelopeFollower::new();
        ef.set_attack_ms(1.0, SR);   // ~1ms attack
        ef.set_release_ms(100.0, SR);

        // Feed constant amplitude for 10ms (441 samples)
        for _ in 0..441 {
            ef.tick(1.0);
        }
        // After 10× the attack time, should be very close to 1.0
        assert_within!(1.0, ef.current(), 0.01);
    }

    #[test]
    fn release_decays_toward_zero() {
        let mut ef = EnvelopeFollower::new();
        ef.set_attack_ms(0.1, SR);
        ef.set_release_ms(10.0, SR);

        // Charge to ~1.0
        for _ in 0..441 {
            ef.tick(1.0);
        }
        let peak = ef.current();
        assert!(peak > 0.9, "should be near 1.0 after charging");

        // Release for 100ms (4410 samples) with silence
        for _ in 0..4410 {
            ef.tick(0.0);
        }
        // Should have decayed significantly
        assert!(ef.current() < 0.1, "should decay after 100ms of silence, got {}", ef.current());
    }

    #[test]
    fn zero_input_stays_at_zero() {
        let mut ef = EnvelopeFollower::new();
        ef.set_attack_ms(5.0, SR);
        ef.set_release_ms(50.0, SR);
        for _ in 0..1000 {
            ef.tick(0.0);
        }
        assert_eq!(0.0, ef.current());
    }

    #[test]
    fn reset_clears_state() {
        let mut ef = EnvelopeFollower::new();
        ef.set_attack_ms(1.0, SR);
        ef.set_release_ms(50.0, SR);
        for _ in 0..441 {
            ef.tick(0.8);
        }
        assert!(ef.current() > 0.5);
        ef.reset();
        assert_eq!(0.0, ef.current());
    }

    #[test]
    fn tracks_rectified_input() {
        let mut ef = EnvelopeFollower::new();
        ef.set_attack_ms(1.0, SR);
        ef.set_release_ms(1.0, SR);

        // Negative input should produce positive envelope (absolute value)
        for _ in 0..441 {
            ef.tick(-0.7);
        }
        assert_within!(0.7, ef.current(), 0.05);
    }

    #[test]
    fn prolonged_silence_does_not_produce_denormals() {
        let mut ef = EnvelopeFollower::new();
        ef.set_attack_ms(1.0, SR);
        ef.set_release_ms(200.0, SR);

        // Charge to ~1.0
        for _ in 0..441 {
            ef.tick(1.0);
        }

        // Release into silence for 30 seconds (~1.3M samples)
        for i in 0..(SR as usize * 30) {
            let val = ef.tick(0.0);
            assert!(
                val == 0.0 || val.is_normal(),
                "denormal at sample {i}: {val:e} (bits: {:#010x})",
                val.to_bits()
            );
        }
    }

    #[test]
    fn fast_attack_slow_release_captures_transient() {
        let mut ef = EnvelopeFollower::new();
        ef.set_attack_ms(0.5, SR);   // very fast attack
        ef.set_release_ms(200.0, SR); // slow release

        // Short transient burst (1ms)
        for _ in 0..44 {
            ef.tick(1.0);
        }
        let after_burst = ef.current();
        assert!(after_burst > 0.5, "should capture transient, got {after_burst}");

        // After 10ms of silence, should still retain significant energy
        for _ in 0..441 {
            ef.tick(0.0);
        }
        assert!(ef.current() > 0.2, "slow release should retain energy, got {}", ef.current());
    }
}
