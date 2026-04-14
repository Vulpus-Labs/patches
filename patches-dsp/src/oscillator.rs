/// A single-channel (mono) phase accumulator.
///
/// Tracks a normalised phase in `[0.0, 1.0)` and a per-sample phase increment.
/// Knows nothing about frequency, sample rate, or modulation — use a frequency
/// converter to compute increments.
pub struct MonoPhaseAccumulator {
    pub phase: f32,
    pub phase_increment: f32,
}

impl MonoPhaseAccumulator {
    pub fn new() -> Self {
        Self { phase: 0.0, phase_increment: 0.0 }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Set the phase increment per sample.
    ///
    /// Clamped below 1.0 (Nyquist) so that [`advance`](Self::advance) can
    /// wrap with a single conditional subtraction.
    pub fn set_increment(&mut self, increment: f32) {
        self.phase_increment = increment.min(0.999_999);
    }

    /// Advance phase by `phase_increment` and wrap to `[0.0, 1.0)`.
    ///
    /// The single conditional subtraction assumes `phase_increment < 1.0`,
    /// which is guaranteed by [`set_increment`].
    pub fn advance(&mut self) {
        let phase = self.phase + self.phase_increment;
        let wrap = if phase >= 1.0 { 1.0 } else { 0.0 };
        self.phase = phase - wrap;
    }
}

impl Default for MonoPhaseAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// A 16-voice polyphonic phase accumulator.
///
/// Tracks normalised phases in `[0.0, 1.0)` and per-sample phase increments
/// for all 16 voices.
///
/// The fixed voice count of 16 allows the compiler to auto-vectorise
/// [`advance_all`](Self::advance_all).
pub struct PolyPhaseAccumulator {
    pub phases: [f32; 16],
    pub phase_increments: [f32; 16],
}

impl PolyPhaseAccumulator {
    pub fn new() -> Self {
        Self {
            phases: [0.0; 16],
            phase_increments: [0.0; 16],
        }
    }

    pub fn reset(&mut self, voice: usize) {
        self.phases[voice] = 0.0;
    }

    pub fn reset_all(&mut self) {
        self.phases = [0.0; 16];
    }

    pub fn set_increment(&mut self, voice: usize, increment: f32) {
        self.phase_increments[voice] = increment;
    }

    /// Set the same phase increment for all 16 voices.
    pub fn set_all_increments(&mut self, increment: f32) {
        self.phase_increments = [increment; 16];
    }

    /// Advance all 16 voices and wrap each to `[0.0, 1.0)`.
    ///
    /// Uses a branchless conditional subtraction over the fixed-size array to
    /// allow auto-vectorisation.
    pub fn advance_all(&mut self) {
        for i in 0..16 {
            let phase = self.phases[i] + self.phase_increments[i];
            let wrap = if phase >= 1.0 { 1.0 } else { 0.0 };
            self.phases[i] = phase - wrap;
        }
    }
}

impl Default for PolyPhaseAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// PolyBLEP correction for a normalised phase `t ∈ [0, 1)` and phase increment `dt`.
///
/// Returns a correction value that smooths the discontinuity near `t = 0` (rising)
/// and `t = 1` (falling) transitions. Only effective when `dt < 0.5`.
pub fn polyblep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T7 — determinism and state reset: two accumulators with the same increment
    /// produce bit-identical phase values after the same number of advance() steps.
    #[test]
    fn t7_two_accumulators_same_increment_are_bit_identical() {
        use crate::test_support::assert_deterministic;

        let increment = 440.0_f32 / 44100.0_f32;
        let dummy: Vec<f32> = vec![0.0; 100];

        assert_deterministic!(
            {
                let mut acc = MonoPhaseAccumulator::new();
                acc.set_increment(increment);
                acc
            },
            &dummy,
            |acc: &mut MonoPhaseAccumulator, _x: f32| { acc.advance(); acc.phase }
        );
    }

    /// T7 — determinism and state reset: after reset(), re-running the same
    /// advance() sequence produces bit-identical results to a fresh instance.
    #[test]
    fn t7_reset_produces_same_sequence_as_fresh_instance() {
        use crate::test_support::assert_reset_deterministic;

        let increment = 440.0_f32 / 44100.0_f32;
        let dummy: Vec<f32> = vec![0.0; 100];

        assert_reset_deterministic!(
            {
                let mut acc = MonoPhaseAccumulator::new();
                acc.set_increment(increment);
                acc
            },
            &dummy,
            |acc: &mut MonoPhaseAccumulator, _x: f32| { acc.advance(); acc.phase },
            |acc: &mut MonoPhaseAccumulator| acc.reset()
        );
    }

    // ── PolyPhaseAccumulator tests ──────────────────────────────────────────

    /// Each voice wraps independently at its own increment rate.
    #[test]
    fn poly_phase_accumulator_wraps_per_voice() {
        let mut poly = PolyPhaseAccumulator::new();
        // Voice 0: fast (wraps in ~10 samples), voice 1: slow (wraps in ~100)
        poly.set_increment(0, 0.1);
        poly.set_increment(1, 0.01);

        let mut wraps_0 = 0usize;
        let mut wraps_1 = 0usize;
        let mut prev_0 = 0.0f32;
        let mut prev_1 = 0.0f32;

        for _ in 0..50 {
            poly.advance_all();
            if poly.phases[0] < prev_0 { wraps_0 += 1; }
            if poly.phases[1] < prev_1 { wraps_1 += 1; }
            prev_0 = poly.phases[0];
            prev_1 = poly.phases[1];
        }

        assert!(wraps_0 > wraps_1, "fast voice should wrap more: {wraps_0} vs {wraps_1}");
        assert!(wraps_0 >= 4, "fast voice should wrap ~5 times in 50 samples, got {wraps_0}");
        assert_eq!(wraps_1, 0, "slow voice should not wrap in 50 samples");
    }

    /// 16 mono accumulators produce the same phases as one poly accumulator.
    #[test]
    fn poly_phase_accumulator_matches_mono() {
        let increments: [f32; 16] = std::array::from_fn(|i| (i as f32 + 1.0) * 440.0 / 44100.0);

        let mut monos: Vec<MonoPhaseAccumulator> = (0..16).map(|i| {
            let mut m = MonoPhaseAccumulator::new();
            m.set_increment(increments[i]);
            m
        }).collect();

        let mut poly = PolyPhaseAccumulator::new();
        for (i, &inc) in increments.iter().enumerate() {
            poly.set_increment(i, inc);
        }

        for step in 0..200 {
            for m in monos.iter_mut() { m.advance(); }
            poly.advance_all();

            for (v, (pp, mm)) in poly.phases.iter().zip(monos.iter()).enumerate() {
                assert_eq!(
                    pp.to_bits(), mm.phase.to_bits(),
                    "voice {v} step {step}: poly={pp} mono={}",
                    mm.phase
                );
            }
        }
    }

    /// Resetting one voice does not affect others.
    #[test]
    fn poly_phase_accumulator_reset_voice() {
        let mut poly = PolyPhaseAccumulator::new();
        for i in 0..16 {
            poly.set_increment(i, 0.05 * (i as f32 + 1.0));
        }
        for _ in 0..20 {
            poly.advance_all();
        }

        let phases_before: [f32; 16] = poly.phases;
        poly.reset(3);

        assert_eq!(poly.phases[3], 0.0, "voice 3 should be reset");
        for (v, (&pp, &pb)) in poly.phases.iter().zip(phases_before.iter()).enumerate() {
            if v != 3 {
                assert_eq!(
                    pp, pb,
                    "voice {v} should be unaffected by resetting voice 3"
                );
            }
        }
    }

    /// Two poly accumulators with the same increments produce bit-identical phases.
    #[test]
    fn poly_phase_accumulator_determinism() {
        let mut a = PolyPhaseAccumulator::new();
        let mut b = PolyPhaseAccumulator::new();
        for i in 0..16 {
            let inc = (i as f32 + 1.0) * 0.003;
            a.set_increment(i, inc);
            b.set_increment(i, inc);
        }
        for step in 0..500 {
            a.advance_all();
            b.advance_all();
            assert_eq!(a.phases, b.phases, "diverged at step {step}");
        }
    }

    /// T2 — frequency response: at 440 Hz with sample_rate=44100, the phase
    /// accumulator wraps exactly once within one period (~100.2 samples).
    /// Verify that exactly one wrap occurs in the first 101 samples.
    #[test]
    fn t2_phase_wraps_once_per_period_at_440hz() {
        let sample_rate = 44100.0_f32;
        let freq = 440.0_f32;
        let increment = freq / sample_rate;

        let mut acc = MonoPhaseAccumulator::new();
        acc.set_increment(increment);

        // One period at 440 Hz = 44100/440 ≈ 100.227 samples.
        // We sample ceil(period)+1 = 102 samples to ensure we capture the wrap.
        let n_samples = 102_usize;
        let mut wrap_count = 0usize;
        let mut prev_phase = acc.phase;

        for _ in 0..n_samples {
            acc.advance();
            // A wrap occurred when phase is less than prev_phase (it subtracted 1.0).
            if acc.phase < prev_phase {
                wrap_count += 1;
            }
            prev_phase = acc.phase;
        }

        assert_eq!(
            wrap_count, 1,
            "expected exactly 1 phase wrap in {n_samples} samples at 440 Hz / 44100 Hz; got {wrap_count}"
        );
    }
}
