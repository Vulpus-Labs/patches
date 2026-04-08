//! Per-tap feedback filter chain used by delay modules.
//!
//! The chain applies in order:
//! 1. Scale by `drive` (controls saturation knee)
//! 2. DC block — one-pole highpass at ~5 Hz
//! 3. HF limiter — one-pole lowpass at ~16 kHz (prevents runaway HF buildup)

use std::f32::consts::TAU;

/// Per-tap feedback filter: DC block → HF limit → fast_tanh saturation.
///
/// Coefficients are computed once in [`prepare`](TapFeedbackFilter::prepare)
/// and remain constant until the sample rate changes. State fields must be
/// zeroed (via [`reset`](TapFeedbackFilter::reset)) whenever the feedback
/// path is interrupted (e.g. on module reinitialisation).
#[derive(Clone)]
pub struct TapFeedbackFilter {
    // DC block state
    x_prev:   f32,
    dc_y_prev: f32,
    // HF limiter state
    hf_y_prev: f32,
    // Coefficients (computed from sample rate)
    /// DC block pole: `R = 1 − 2π·5 / sample_rate`
    r:     f32,
    /// HF limiter: `α = 1 − exp(−2π·16000 / sample_rate)`
    alpha: f32,
}

impl TapFeedbackFilter {
    pub fn new() -> Self {
        Self {
            x_prev:    0.0,
            dc_y_prev: 0.0,
            hf_y_prev: 0.0,
            r:         0.9993, // ~5 Hz at 44.1 kHz, safe default
            alpha:     1.0,
        }
    }

    /// Compute coefficients for the given sample rate.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.r     = 1.0 - TAU * 5.0 / sample_rate;
        self.alpha = 1.0 - (-TAU * 16_000.0 / sample_rate).exp();
    }

    /// Zero all filter state. Call when the feedback path is interrupted.
    pub fn reset(&mut self) {
        self.x_prev    = 0.0;
        self.dc_y_prev = 0.0;
        self.hf_y_prev = 0.0;
    }

    /// Process one sample through the feedback chain.
    ///
    /// 1. `x *= drive`
    /// 2. DC block: `y = x − x_prev + R·y_prev`
    /// 3. HF limit: `y = y_prev + α·(y − y_prev)`
    /// 4. return `y`
    #[inline]
    pub fn process(&mut self, x: f32, drive: f32) -> f32 {
        let x_driven = x * drive;

        // DC block (one-pole highpass ~5 Hz)
        let dc_y = x_driven - self.x_prev + self.r * self.dc_y_prev;
        self.x_prev    = x_driven;
        self.dc_y_prev = dc_y;

        // HF limiter (one-pole lowpass ~16 kHz)
        let hf_y = self.hf_y_prev + self.alpha * (dc_y - self.hf_y_prev);
        self.hf_y_prev = hf_y;

        hf_y
    }
}

impl Default for TapFeedbackFilter {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{assert_within, assert_reset_deterministic, sine_rms_warmed};

    const SR: f32 = 44_100.0;

    fn prepared() -> TapFeedbackFilter {
        let mut f = TapFeedbackFilter::new();
        f.prepare(SR);
        f
    }

    #[test]
    fn dc_input_converges_to_zero() {
        // A sustained DC input must be blocked by the highpass.
        let mut f = prepared();
        let mut out = 0.0_f32;
        for _ in 0..44_100 {
            out = f.process(0.5, 1.0);
        }
        assert_within!(0.0, out, 1e-3);
    }

    #[test]
    fn drive_scales_output_linearly() {
        // Without saturation in the feedback filter, drive multiplies the output.
        let mut f1 = prepared();
        let mut f2 = prepared();
        let out1 = f1.process(0.1, 1.0);
        let out2 = f2.process(0.1, 2.0);
        assert_within!(2.0 * out1, out2, 1e-6);
    }

    #[test]
    fn zero_input_produces_zero() {
        let mut f = prepared();
        let out = f.process(0.0, 1.0);
        assert_eq!(out, 0.0);
    }

    // ── T2: frequency-response spot checks ──────────────────────────────────

    /// Passband gain at a low frequency (100 Hz) should be ≈ 1.0.
    /// The DC block cutoff is ~5 Hz and the HF limiter cutoff is ~16 kHz,
    /// so 100 Hz is solidly in the passband.
    #[test]
    fn passband_gain_near_unity_at_100hz() {
        let mut f = prepared();
        let rms = sine_rms_warmed(100.0, SR, 44_100, 44_100, |x| f.process(x, 1.0));
        // The expected RMS of a sine is 1/√2 ≈ 0.707; allow ±5 %
        assert_within!(0.707, rms, 0.040, "passband gain should be ≈ 1 at 100 Hz, rms={rms}");
    }

    /// At 1 kHz (well within passband) the filter should also be near unity.
    #[test]
    fn passband_gain_near_unity_at_1khz() {
        let mut f = prepared();
        let rms = sine_rms_warmed(1_000.0, SR, 44_100, 44_100, |x| f.process(x, 1.0));
        assert_within!(0.707, rms, 0.040, "passband gain should be ≈ 1 at 1 kHz, rms={rms}");
    }

    /// At 10 kHz the HF limiter starts to roll off, so the output should have
    /// less energy than the 100 Hz passband case.
    #[test]
    fn hf_limiter_attenuates_above_passband() {
        let mut f_lo = prepared();
        let rms_100 = sine_rms_warmed(100.0, SR, 44_100, 44_100, |x| f_lo.process(x, 1.0));

        let mut f_hi = prepared();
        let rms_10k = sine_rms_warmed(10_000.0, SR, 44_100, 44_100, |x| f_hi.process(x, 1.0));

        // The HF limiter cutoff is ~16 kHz; at 10 kHz some attenuation is expected
        // but not dramatic. The low-frequency signal should be at least as loud.
        assert!(
            rms_100 >= rms_10k,
            "low-frequency passband should have at least as much energy as 10 kHz, \
             rms_100={rms_100} rms_10k={rms_10k}"
        );
    }

    // ── T4: stability under maximum amplitude + maximum drive ───────────────

    /// Drive maximum-amplitude input with a large drive value for 10 000 samples.
    /// Output must remain finite (no NaN or infinity).
    #[test]
    fn stable_under_max_amplitude_and_drive() {
        use std::f32::consts::TAU;
        const DRIVE: f32 = 10.0;
        const N: usize = 10_000;
        let mut f = prepared();
        let inc = 440.0_f32 / SR;
        for i in 0..N {
            let x = (i as f32 * inc * TAU).sin(); // amplitude = 1.0
            let y = f.process(x, DRIVE);
            assert!(y.is_finite(), "output not finite at sample {i}: y={y}");
        }
    }

    // ── T7: state reset produces bit-identical output ────────────────────────

    /// Processing the same input sequence twice with a state reset between runs
    /// must yield bit-identical output.
    #[test]
    fn state_reset_produces_identical_output() {
        use std::f32::consts::TAU;
        const FREQ: f32 = 440.0;
        const N: usize = 1_000;
        let inc = FREQ / SR;

        let input: Vec<f32> = (0..N)
            .map(|i| (i as f32 * inc * TAU).sin())
            .collect();

        assert_reset_deterministic!(
            prepared(),
            &input,
            |f: &mut TapFeedbackFilter, x: f32| f.process(x, 1.0),
            |f: &mut TapFeedbackFilter| f.reset()
        );
    }
}
