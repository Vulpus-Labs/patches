//! Feed-forward compressor detector kernel.
//!
//! Converts a detector input sample into a linear gain applied to the dry
//! path. Caller is responsible for selecting the detector source (self-key
//! vs sidechain) and, for stereo, linking L/R into a single magnitude
//! before calling [`CompDetector::tick`].
//!
//! Algorithm:
//! 1. Rectify / square the input depending on [`DetectMode`].
//! 2. Smooth with an asymmetric one-pole (attack on rising, release on
//!    falling).
//! 3. Convert envelope → dB level.
//! 4. Apply the static gain function (threshold + ratio + soft knee) to
//!    obtain the gain-reduction amount in dB.
//! 5. Convert reduction back to a linear gain and apply makeup.
//!
//! The static gain function is C¹-continuous across the knee region: at
//! both `threshold ± knee_width / 2` the value and slope match the
//! adjacent below-knee / above-knee regions, so the knee introduces no
//! kink.

use patches_dsp::compute_time_coeff;

/// Detection mode for [`CompDetector`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectMode {
    /// Envelope tracks `|x|`.
    Peak,
    /// Envelope tracks `x²`; level is `sqrt(envelope)`.
    Rms,
}

/// Pure DSP kernel: detector input sample → linear output gain.
#[derive(Clone, Debug)]
pub struct CompDetector {
    sample_rate: f32,
    threshold_db: f32,
    ratio: f32,
    knee_width_db: f32,
    makeup_lin: f32,
    mode: DetectMode,
    attack_coeff: f32,
    release_coeff: f32,
    envelope: f32,
}

impl CompDetector {
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            sample_rate,
            threshold_db: -12.0,
            ratio: 4.0,
            knee_width_db: 6.0,
            makeup_lin: 1.0,
            mode: DetectMode::Peak,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            envelope: 0.0,
        };
        s.set_attack_ms(10.0);
        s.set_release_ms(100.0);
        s
    }

    pub fn set_threshold_db(&mut self, db: f32) {
        self.threshold_db = db;
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.max(1.0);
    }

    pub fn set_knee_width_db(&mut self, w: f32) {
        self.knee_width_db = w.max(0.0);
    }

    pub fn set_makeup_db(&mut self, db: f32) {
        self.makeup_lin = db_to_lin(db);
    }

    pub fn set_mode(&mut self, mode: DetectMode) {
        if mode != self.mode {
            self.envelope = 0.0;
        }
        self.mode = mode;
    }

    pub fn set_attack_ms(&mut self, ms: f32) {
        self.attack_coeff = compute_time_coeff(ms, self.sample_rate);
    }

    pub fn set_release_ms(&mut self, ms: f32) {
        self.release_coeff = compute_time_coeff(ms, self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
    }

    /// Process one detector sample. Returns linear gain (post-makeup) to
    /// apply to the dry path. The input may be signed — it is rectified
    /// internally.
    #[inline]
    pub fn tick(&mut self, key: f32) -> f32 {
        let abs_key = key.abs();
        let target = match self.mode {
            DetectMode::Peak => abs_key,
            DetectMode::Rms => abs_key * abs_key,
        };
        let coeff = if target > self.envelope { self.attack_coeff } else { self.release_coeff };
        self.envelope += coeff * (target - self.envelope);
        let level = match self.mode {
            DetectMode::Peak => self.envelope,
            DetectMode::Rms => self.envelope.max(0.0).sqrt(),
        };
        let level_db = lin_to_db(level);
        let gr_db = self.gain_reduction_db(level_db);
        db_to_lin(-gr_db) * self.makeup_lin
    }

    /// Static gain-reduction curve in dB. `slope = 1 - 1/ratio`; for
    /// `ratio = ∞` the slope collapses to `1` (limiter shape) because
    /// `1 / f32::INFINITY == 0`.
    fn gain_reduction_db(&self, level_db: f32) -> f32 {
        let slope = 1.0 - 1.0 / self.ratio;
        let w = self.knee_width_db;
        if w <= 0.0 {
            return (level_db - self.threshold_db).max(0.0) * slope;
        }
        let lo = self.threshold_db - 0.5 * w;
        let hi = self.threshold_db + 0.5 * w;
        if level_db <= lo {
            0.0
        } else if level_db >= hi {
            (level_db - self.threshold_db) * slope
        } else {
            // Quadratic in the knee region. Value and first derivative
            // match the linear branch at L = hi and the zero branch at
            // L = lo, so the curve is C¹-continuous.
            let t = level_db - lo;
            slope * t * t / (2.0 * w)
        }
    }
}

#[inline]
fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(1.0e-10).log10()
}

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn new_detector() -> CompDetector {
        let mut d = CompDetector::new(SR);
        d.set_threshold_db(-12.0);
        d.set_ratio(4.0);
        d.set_knee_width_db(0.0);
        d.set_attack_ms(10.0);
        d.set_release_ms(100.0);
        d
    }

    #[test]
    fn below_threshold_is_unity() {
        let mut d = new_detector();
        // -20 dBFS is well below -12 dB threshold.
        let lvl = db_to_lin(-20.0);
        let mut g = 0.0;
        for _ in 0..2_000 {
            g = d.tick(lvl);
        }
        assert!((g - 1.0).abs() < 1e-3, "expected unity gain below threshold, got {g}");
    }

    #[test]
    fn static_curve_at_threshold_unity_hard_knee() {
        // Hard knee: at the threshold the gain reduction is exactly 0.
        let d = new_detector();
        let gr = d.gain_reduction_db(-12.0);
        assert_eq!(gr, 0.0);
    }

    #[test]
    fn static_curve_above_threshold_linear_slope() {
        // Hard knee, ratio = 4 → slope = 0.75. At +12 dB above threshold
        // gain reduction is 9 dB.
        let d = new_detector();
        let gr = d.gain_reduction_db(0.0);
        assert!((gr - 9.0).abs() < 1e-4, "expected 9 dB reduction, got {gr}");
    }

    #[test]
    fn soft_knee_is_c1_continuous() {
        // Sample the curve and verify both value-continuity and
        // slope-continuity across the knee boundaries.
        let mut d = new_detector();
        d.set_threshold_db(-12.0);
        d.set_knee_width_db(6.0);
        d.set_ratio(4.0);

        let lo = -12.0 - 3.0; // threshold - knee/2
        let hi = -12.0 + 3.0; // threshold + knee/2
        let eps = 1.0e-4_f32;

        // Value continuity at both boundaries — the function value differs
        // across the boundary by at most `slope * 2 * eps`; tolerate that.
        let slope = 1.0 - 1.0 / 4.0;
        let value_tol = slope * 4.0 * eps;
        let v_lo_in = d.gain_reduction_db(lo + eps);
        let v_lo_out = d.gain_reduction_db(lo - eps);
        assert!(
            (v_lo_in - v_lo_out).abs() < value_tol,
            "value jump at lo: {v_lo_in} vs {v_lo_out}"
        );
        let v_hi_in = d.gain_reduction_db(hi - eps);
        let v_hi_out = d.gain_reduction_db(hi + eps);
        assert!(
            (v_hi_in - v_hi_out).abs() < value_tol,
            "value jump at hi: {v_hi_in} vs {v_hi_out}"
        );

        // Numerical slope on either side of each boundary.
        let h = 1.0e-3_f32;
        let slope_at = |d: &CompDetector, x: f32| {
            (d.gain_reduction_db(x + h) - d.gain_reduction_db(x - h)) / (2.0 * h)
        };
        let s_lo_in = slope_at(&d, lo + 5.0 * h);
        let s_lo_out = slope_at(&d, lo - 5.0 * h);
        assert!(
            (s_lo_in - s_lo_out).abs() < 1.0e-2,
            "slope jump at lo: in={s_lo_in}, out={s_lo_out}"
        );
        let s_hi_in = slope_at(&d, hi - 5.0 * h);
        let s_hi_out = slope_at(&d, hi + 5.0 * h);
        assert!(
            (s_hi_in - s_hi_out).abs() < 1.0e-2,
            "slope jump at hi: in={s_hi_in}, out={s_hi_out}"
        );
    }

    /// Settle the detector with a constant rectified input and return the
    /// observed envelope level (in linear units, post-ballistics).
    fn settled_level(d: &mut CompDetector, key: f32, n: usize) -> f32 {
        for _ in 0..n {
            d.tick(key);
        }
        match d.mode {
            DetectMode::Peak => d.envelope,
            DetectMode::Rms => d.envelope.max(0.0).sqrt(),
        }
    }

    #[test]
    fn peak_ballistics_attack_within_five_percent() {
        // After 5× the attack time constant a one-pole follower has
        // reached `1 - exp(-5) ≈ 0.9933` of the target.
        let mut d = new_detector();
        d.set_attack_ms(10.0);
        d.set_release_ms(1000.0);
        let target = 0.5_f32;
        let attack_samples = (10.0 * 0.001 * SR) as usize;
        let n = 5 * attack_samples;
        let lvl = settled_level(&mut d, target, n);
        let expected = (1.0 - (-5.0_f32).exp()) * target;
        let rel_err = (lvl - expected).abs() / expected;
        assert!(rel_err < 0.05, "attack ballistics off: lvl={lvl}, expected={expected}");
    }

    #[test]
    fn peak_ballistics_release_within_five_percent() {
        // Charge to 1.0, then release into silence for 5× release time.
        let mut d = new_detector();
        d.set_attack_ms(0.5);
        d.set_release_ms(50.0);
        for _ in 0..((0.5 * 0.001 * SR) as usize * 20) {
            d.tick(1.0);
        }
        let peak = d.envelope;
        assert!(peak > 0.95, "didn't charge: {peak}");

        let release_samples = (50.0 * 0.001 * SR) as usize;
        for _ in 0..(5 * release_samples) {
            d.tick(0.0);
        }
        let expected = peak * (-5.0_f32).exp();
        let lvl = d.envelope;
        assert!(
            (lvl - expected).abs() / expected.max(1.0e-6) < 0.05,
            "release ballistics off: lvl={lvl}, expected={expected}"
        );
    }

    #[test]
    fn ratio_infinity_saturates_to_threshold() {
        // Limiter shape: large input must produce output level ≈ threshold,
        // regardless of how far above the threshold we push.
        let mut d = new_detector();
        d.set_threshold_db(-6.0);
        d.set_ratio(f32::INFINITY);
        d.set_knee_width_db(0.0);
        d.set_attack_ms(0.5);
        d.set_release_ms(50.0);

        let threshold_lin = db_to_lin(-6.0);
        for input_db in [0.0_f32, 6.0, 12.0, 24.0] {
            d.reset();
            let lvl = db_to_lin(input_db);
            // Settle: feed constant rectified input until envelope tracks it.
            let mut gain = 1.0;
            for _ in 0..10_000 {
                gain = d.tick(lvl);
            }
            let out = lvl * gain;
            let rel_err = (out - threshold_lin).abs() / threshold_lin;
            assert!(
                rel_err < 0.02,
                "ratio=inf failed asymptote at {input_db} dB: out={out}, threshold_lin={threshold_lin}"
            );
        }
    }

    #[test]
    fn rms_and_peak_differ_on_transient() {
        // Same time constants, same input: the peak and RMS detector paths
        // must produce visibly different gain reduction. (Direction is not
        // asserted — with matched attack times the RMS path's `sqrt` of a
        // partial envelope rises faster in level terms than peak's
        // `|x|` smoothing, but the point is they differ.)
        let make = |mode: DetectMode| {
            let mut d = new_detector();
            d.set_threshold_db(-20.0);
            d.set_ratio(8.0);
            d.set_knee_width_db(0.0);
            d.set_attack_ms(5.0);
            d.set_release_ms(50.0);
            d.set_mode(mode);
            d
        };
        let mut peak = make(DetectMode::Peak);
        let mut rms = make(DetectMode::Rms);

        // 1 ms burst at +6 dB (lin = 2.0).
        let burst_len = (1.0 * 0.001 * SR) as usize;
        let mut g_peak = 1.0;
        let mut g_rms = 1.0;
        for _ in 0..burst_len {
            g_peak = peak.tick(2.0);
            g_rms = rms.tick(2.0);
        }
        assert!(
            (g_peak - g_rms).abs() > 0.05,
            "peak vs rms should differ on a transient: peak={g_peak}, rms={g_rms}"
        );
    }
}
