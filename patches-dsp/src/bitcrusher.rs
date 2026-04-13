//! Sample-rate reduction and bit-depth reduction DSP kernel.
//!
//! The kernel implements two independent degradation effects:
//!
//! - **Rate reduction** via a sample-and-hold with a fractional phase
//!   accumulator. When the phase wraps past 1.0, a new input sample is
//!   captured; otherwise the held value is returned.
//!
//! - **Bit reduction** via uniform quantisation: `round(x * levels) / levels`
//!   where `levels = 2^depth`. Continuous (non-integer) depth values produce
//!   smooth degradation.

/// Sample-rate and bit-depth reduction kernel.
///
/// Zero-allocation: only a held sample value, phase accumulator, and cached
/// parameters.
#[derive(Clone)]
pub struct BitcrusherKernel {
    /// Current held sample (output when phase hasn't wrapped).
    held: f32,
    /// Fractional phase accumulator for rate reduction.
    phase: f32,
    /// Phase increment per tick: `effective_rate / sample_rate`.
    phase_inc: f32,
    /// Quantisation levels: `2^depth`.
    levels: f32,
}

impl BitcrusherKernel {
    pub fn new() -> Self {
        Self {
            held: 0.0,
            phase: 1.0, // start at 1.0 so first tick always captures
            phase_inc: 1.0, // 1.0 = full rate (no reduction)
            levels: f32::MAX, // effectively no quantisation
        }
    }

    /// Set the rate reduction amount.
    ///
    /// `rate` is in [0.0, 1.0] where 1.0 = full sample rate (no reduction)
    /// and 0.0 = minimum effective rate (~100 Hz).
    ///
    /// Uses logarithmic mapping: `effective_rate = 100 * (sr / 100)^rate`.
    pub fn set_rate(&mut self, rate: f32, sample_rate: f32) {
        let rate = rate.clamp(0.0, 1.0);
        let effective_rate = 100.0 * (sample_rate / 100.0).powf(rate);
        self.phase_inc = (effective_rate / sample_rate).clamp(0.0, 1.0);
    }

    /// Set the bit depth for quantisation.
    ///
    /// `depth` is in [1.0, 32.0]. At 32.0 the quantisation is inaudible;
    /// at 1.0 the signal snaps to two levels.
    pub fn set_depth(&mut self, depth: f32) {
        let depth = depth.clamp(1.0, 32.0);
        // For depth >= 24.0, disable quantisation to avoid float precision issues.
        if depth >= 24.0 {
            self.levels = f32::MAX;
        } else {
            self.levels = (2.0_f32).powf(depth);
        }
    }

    /// Process one sample.
    #[inline]
    pub fn tick(&mut self, input: f32) -> f32 {
        self.phase += self.phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.held = self.quantize(input);
        }
        self.held
    }

    /// Reset all state to initial values.
    pub fn reset(&mut self) {
        self.held = 0.0;
        self.phase = 1.0; // ensures next tick captures immediately
    }

    /// Quantise a sample to the configured bit depth.
    ///
    /// Returns `x` unchanged when depth >= 24 (levels = `f32::MAX`).
    #[inline]
    pub fn quantize(&self, x: f32) -> f32 {
        if self.levels >= f32::MAX {
            return x;
        }
        (x * self.levels).round() / self.levels
    }
}

impl Default for BitcrusherKernel {
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
    fn full_rate_full_depth_is_identity() {
        let mut k = BitcrusherKernel::new();
        k.set_rate(1.0, SR);
        k.set_depth(32.0);
        let input = [0.1, -0.5, 0.9, -0.3, 0.0];
        for &x in &input {
            let y = k.tick(x);
            assert_eq!(x, y, "full rate + full depth should pass through unchanged");
        }
    }

    #[test]
    fn zero_rate_holds_first_sample() {
        let mut k = BitcrusherKernel::new();
        k.set_rate(0.0, SR);
        k.set_depth(32.0);
        // At rate=0, effective rate is ~100 Hz. Phase inc ≈ 100/44100 ≈ 0.0023.
        // For the first ~440 samples, the held value should remain the same.
        let first = k.tick(0.75);
        assert_within!(0.75, first, 1e-6);
        for i in 1..400 {
            let y = k.tick(i as f32 * 0.001);
            assert_within!(0.75, y, 1e-6, "sample {i} should still hold first value");
        }
    }

    #[test]
    fn one_bit_quantises_to_two_levels() {
        let mut k = BitcrusherKernel::new();
        k.set_rate(1.0, SR);
        k.set_depth(1.0);
        // 2^1 = 2 levels. round(x * 2) / 2 gives {-1.0, -0.5, 0.0, 0.5, 1.0}
        let y_pos = k.tick(0.3);
        assert_within!(0.5, y_pos, 1e-6, "0.3 should quantise to 0.5 at 1-bit");
        let y_neg = k.tick(-0.8);
        assert_within!(-1.0, y_neg, 1e-6, "-0.8 should quantise to -1.0 at 1-bit");
        let y_zero = k.tick(0.1);
        assert_within!(0.0, y_zero, 1e-6, "0.1 should quantise to 0.0 at 1-bit");
    }

    #[test]
    fn reset_clears_state() {
        let mut k = BitcrusherKernel::new();
        k.set_rate(1.0, SR);
        k.set_depth(32.0);
        k.tick(0.5);
        k.tick(0.7);
        k.reset();
        assert_eq!(0.0, k.held);
        assert_eq!(1.0, k.phase);
    }

    #[test]
    fn low_depth_quantisation_stays_finite() {
        let mut k = BitcrusherKernel::new();
        k.set_rate(1.0, SR);
        // Sweep depth from 1.0 to 32.0 in fine increments
        for d in 0..320 {
            let depth = 1.0 + d as f32 * 0.1;
            k.set_depth(depth);
            for &input in &[-1.0, -0.5, 0.0, 0.001, 0.5, 1.0, f32::MIN_POSITIVE] {
                let out = k.tick(input);
                assert!(
                    out.is_finite(),
                    "non-finite output at depth={depth}, input={input}: {out}"
                );
            }
        }
    }

    #[test]
    fn rate_reduction_produces_staircase() {
        let mut k = BitcrusherKernel::new();
        // Set rate so that roughly every 4th sample is captured
        // effective_rate = sr / 4 → phase_inc = 0.25
        k.phase_inc = 0.25;
        k.set_depth(32.0);

        let input: Vec<f32> = (0..16).map(|i| i as f32 * 0.1).collect();
        let output: Vec<f32> = input.iter().map(|&x| k.tick(x)).collect();

        // Output should have staircase pattern — groups of ~4 identical values
        // First tick captures sample 0
        assert_within!(0.0, output[0], 1e-6);
        assert_within!(0.0, output[1], 1e-6);
        assert_within!(0.0, output[2], 1e-6);
        // Around sample 4, a new value should be captured
        assert!(output[4] > 0.0 || output[3] > 0.0, "should capture a new sample by tick 4");
    }
}
