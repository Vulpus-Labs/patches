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

    /// Advance and return the sub-sample wrap position in `(0, 1]`, or `0.0`
    /// if no wrap occurred this sample (ADR 0047 `Trigger` encoding).
    pub fn advance_wrap_frac(&mut self) -> f32 {
        let dt = self.phase_increment;
        let next = self.phase + dt;
        if next >= 1.0 {
            self.phase = next - 1.0;
            if dt > 0.0 {
                (1.0 - self.phase / dt).clamp(f32::MIN_POSITIVE, 1.0)
            } else {
                1.0
            }
        } else {
            self.phase = next;
            0.0
        }
    }

    /// Reset the phase for a sub-sample sync event at fractional position
    /// `frac ∈ (0, 1]`. Sets phase to `(1 - frac) * phase_increment` so the
    /// remainder of the current sample is accounted for.
    pub fn sync_reset(&mut self, frac: f32) {
        let frac = frac.clamp(f32::MIN_POSITIVE, 1.0);
        self.phase = (1.0 - frac) * self.phase_increment;
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

    /// Advance all voices and return per-voice sub-sample wrap positions
    /// (ADR 0047 `Trigger` encoding; `0.0` = no wrap).
    pub fn advance_all_wrap_frac(&mut self) -> [f32; 16] {
        let mut out = [0.0_f32; 16];
        for (i, slot) in out.iter_mut().enumerate() {
            let dt = self.phase_increments[i];
            let next = self.phases[i] + dt;
            if next >= 1.0 {
                self.phases[i] = next - 1.0;
                *slot = if dt > 0.0 {
                    (1.0 - self.phases[i] / dt).clamp(f32::MIN_POSITIVE, 1.0)
                } else {
                    1.0
                };
            } else {
                self.phases[i] = next;
            }
        }
        out
    }

    /// Reset a single voice's phase for a sub-sample sync event.
    pub fn sync_reset(&mut self, voice: usize, frac: f32) {
        let frac = frac.clamp(f32::MIN_POSITIVE, 1.0);
        self.phases[voice] = (1.0 - frac) * self.phase_increments[voice];
    }
}

impl Default for PolyPhaseAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// Q32 → cycles scale: `2^32`. A full `u32` range is one cycle.
const Q32_SCALE: f32 = 4_294_967_296.0;

/// A single-channel (mono) **Q32 fixed-point** phase accumulator (ADR 0080).
///
/// The full `u32` range maps to one cycle, so the wrap at `2^32` is free
/// (`wrapping_add`, no conditional). Unlike [`MonoPhaseAccumulator`], phase
/// resolution is uniform across the cycle — there is no f32 ulp growth near
/// `phase = 1.0`, which is the defect this type exists to fix for very slow
/// (sub-`0.1` Hz) modulation. Tuning is unchanged: the increment is still
/// derived from an f32 `frequency/sample_rate`; only *accumulation* is exact.
///
/// Phase is read as `u32` so consumers can `wrapping_add` a modulation offset
/// before lookup (e.g. phase-mod / FM); [`lookup_sine_q32`] is the table reader.
///
/// [`lookup_sine_q32`]: crate::lookup_sine_q32
pub struct MonoPhaseAccumulatorQ32 {
    pub phase: u32,
    pub increment: u32,
}

impl MonoPhaseAccumulatorQ32 {
    pub fn new() -> Self {
        Self { phase: 0, increment: 0 }
    }

    pub fn reset(&mut self) {
        self.phase = 0;
    }

    /// Set the per-sample increment from cycles/sample (the same
    /// `frequency/sample_rate` value the f32 accumulator takes). Clamped to
    /// `[0.0, 0.999_999]` before scaling by `2^32`; the clamp matches the f32
    /// path's sub-Nyquist guarantee and keeps the float→u32 cast in range.
    pub fn set_increment(&mut self, increment: f32) {
        let frac = increment.clamp(0.0, 0.999_999);
        self.increment = (frac * Q32_SCALE) as u32;
    }

    /// Advance phase by `increment`; wraps free at `2^32`.
    pub fn advance(&mut self) {
        self.phase = self.phase.wrapping_add(self.increment);
    }

    /// Current phase as a normalised f32 in `[0, 1)` (introspection / analytic
    /// shapes that still want a float).
    pub fn phase_f32(&self) -> f32 {
        self.phase as f32 / Q32_SCALE
    }

    /// Reset the phase for a sub-sample sync event at fractional position
    /// `frac ∈ (0, 1]`, accounting for the remainder of the current sample.
    /// Mirrors [`MonoPhaseAccumulator::sync_reset`] in the Q32 domain.
    pub fn sync_reset(&mut self, frac: f32) {
        let frac = frac.clamp(f32::MIN_POSITIVE, 1.0);
        self.phase = ((1.0 - frac) * self.increment as f32) as u32;
    }
}

impl Default for MonoPhaseAccumulatorQ32 {
    fn default() -> Self {
        Self::new()
    }
}

/// A 16-voice polyphonic **Q32 fixed-point** phase accumulator (ADR 0080).
///
/// The Q32 counterpart of [`PolyPhaseAccumulator`]: `u32` phase and increment
/// per voice, free wrap at `2^32`. The fixed voice count of 16 lets the
/// compiler auto-vectorise [`advance_all`](Self::advance_all) (plain `u32`
/// adds, which vectorise cleanly).
pub struct PolyPhaseAccumulatorQ32 {
    pub phases: [u32; 16],
    pub increments: [u32; 16],
}

impl PolyPhaseAccumulatorQ32 {
    pub fn new() -> Self {
        Self { phases: [0; 16], increments: [0; 16] }
    }

    pub fn reset(&mut self, voice: usize) {
        self.phases[voice] = 0;
    }

    pub fn reset_all(&mut self) {
        self.phases = [0; 16];
    }

    pub fn set_increment(&mut self, voice: usize, increment: f32) {
        let frac = increment.clamp(0.0, 0.999_999);
        self.increments[voice] = (frac * Q32_SCALE) as u32;
    }

    /// Set the same increment for all 16 voices.
    pub fn set_all_increments(&mut self, increment: f32) {
        let frac = increment.clamp(0.0, 0.999_999);
        self.increments = [(frac * Q32_SCALE) as u32; 16];
    }

    /// Advance all 16 voices; each wraps free at `2^32`.
    pub fn advance_all(&mut self) {
        for i in 0..16 {
            self.phases[i] = self.phases[i].wrapping_add(self.increments[i]);
        }
    }

    /// Voice `voice`'s phase as a normalised f32 in `[0, 1)`.
    pub fn phase_f32(&self, voice: usize) -> f32 {
        self.phases[voice] as f32 / Q32_SCALE
    }

    /// Reset a single voice's phase for a sub-sample sync event.
    pub fn sync_reset(&mut self, voice: usize, frac: f32) {
        let frac = frac.clamp(f32::MIN_POSITIVE, 1.0);
        self.phases[voice] = ((1.0 - frac) * self.increments[voice] as f32) as u32;
    }
}

impl Default for PolyPhaseAccumulatorQ32 {
    fn default() -> Self {
        Self::new()
    }
}

/// PolyBLEP correction for a normalised phase `t ∈ [0, 1)` and phase increment `dt`.
///
/// Returns a correction value that smooths the discontinuity near `t = 0` (rising)
/// and `t = 1` (falling) transitions. Only effective when `dt < 0.5`.
///
/// Hard-sync callers build a 2-point residual directly from this function (see
/// `Oscillator`/`PolyOsc`): the leading half at `polyblep(1 - frac·dt, dt)` and
/// the trailing half at `polyblep((1 - frac)·dt, dt)`, each scaled by
/// `-0.5 · (pre - post)` to match the free path's `value - polyblep(...)` sign.
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

    // ── Q32 accumulator tests (ADR 0080) ─────────────────────────────────────

    /// Determinism: two Q32 accumulators with the same increment produce a
    /// bit-identical phase sequence.
    #[test]
    fn q32_mono_determinism() {
        let mut a = MonoPhaseAccumulatorQ32::new();
        let mut b = MonoPhaseAccumulatorQ32::new();
        a.set_increment(440.0 / 44100.0);
        b.set_increment(440.0 / 44100.0);
        for step in 0..1000 {
            a.advance();
            b.advance();
            assert_eq!(a.phase, b.phase, "diverged at step {step}");
        }
    }

    /// Free wrap: phase wraps at 2^32 with no conditional. A quarter-cycle
    /// increment returns to 0 after exactly 4 advances.
    #[test]
    fn q32_mono_free_wrap_at_2_pow_32() {
        let mut acc = MonoPhaseAccumulatorQ32::new();
        acc.increment = 0x4000_0000; // exactly a quarter cycle
        acc.advance();
        assert_eq!(acc.phase, 0x4000_0000);
        acc.advance();
        assert_eq!(acc.phase, 0x8000_0000);
        acc.advance();
        assert_eq!(acc.phase, 0xC000_0000);
        acc.advance(); // wraps back to 0 with no branch
        assert_eq!(acc.phase, 0);
    }

    /// Low-rate no-absorption — the f32 defect this type exists to fix. At
    /// 0.001 Hz and 0.01 Hz / 48 kHz the increment is non-zero and the phase
    /// advances by exactly that increment every sample (no stall near the top
    /// of the cycle, where f32 absorbs the increment into its ulp).
    #[test]
    fn q32_mono_no_low_rate_absorption() {
        for rate_hz in [0.001_f32, 0.01] {
            let mut acc = MonoPhaseAccumulatorQ32::new();
            acc.set_increment(rate_hz / 48_000.0);
            assert!(acc.increment > 0, "{rate_hz} Hz increment was absorbed to 0");

            let inc = acc.increment;
            let mut prev = acc.phase;
            for sample in 0..20_000 {
                acc.advance();
                assert_eq!(
                    acc.phase,
                    prev.wrapping_add(inc),
                    "{rate_hz} Hz stalled at sample {sample}"
                );
                assert!(acc.phase > prev, "{rate_hz} Hz not strictly increasing");
                prev = acc.phase;
            }
        }
    }

    /// `set_increment` parity: the Q32 increment is the f32 increment scaled by
    /// 2^32 (within the truncation of the cast).
    #[test]
    fn q32_set_increment_scales_to_2_pow_32() {
        let mut acc = MonoPhaseAccumulatorQ32::new();
        acc.set_increment(0.25);
        assert_eq!(acc.increment, 0x4000_0000);
        acc.set_increment(0.5);
        assert_eq!(acc.increment, 0x8000_0000);
    }

    /// `sync_reset` lands the sub-sample phase `(1 - frac) * increment`.
    #[test]
    fn q32_mono_sync_reset_sub_sample() {
        let mut acc = MonoPhaseAccumulatorQ32::new();
        acc.increment = 4000;
        acc.phase = 12345; // dirty
        acc.sync_reset(0.25);
        assert_eq!(acc.phase, ((1.0 - 0.25) * 4000.0) as u32); // 3000
        acc.sync_reset(1.0);
        assert_eq!(acc.phase, 0); // whole sample boundary
    }

    /// `reset` zeroes the phase.
    #[test]
    fn q32_mono_reset() {
        let mut acc = MonoPhaseAccumulatorQ32::new();
        acc.set_increment(0.1);
        for _ in 0..5 {
            acc.advance();
        }
        assert_ne!(acc.phase, 0);
        acc.reset();
        assert_eq!(acc.phase, 0);
    }

    /// Poly Q32 matches 16 mono Q32 accumulators (bit-identical per voice).
    #[test]
    fn q32_poly_matches_mono() {
        let increments: [f32; 16] =
            std::array::from_fn(|i| (i as f32 + 1.0) * 440.0 / 44100.0);

        let mut monos: Vec<MonoPhaseAccumulatorQ32> = (0..16)
            .map(|i| {
                let mut m = MonoPhaseAccumulatorQ32::new();
                m.set_increment(increments[i]);
                m
            })
            .collect();

        let mut poly = PolyPhaseAccumulatorQ32::new();
        for (i, &inc) in increments.iter().enumerate() {
            poly.set_increment(i, inc);
        }

        for step in 0..500 {
            for m in monos.iter_mut() {
                m.advance();
            }
            poly.advance_all();
            for (v, (pp, mm)) in poly.phases.iter().zip(monos.iter()).enumerate() {
                assert_eq!(*pp, mm.phase, "voice {v} step {step}");
            }
        }
    }

    /// Resetting one poly voice leaves the others untouched.
    #[test]
    fn q32_poly_reset_voice_isolation() {
        let mut poly = PolyPhaseAccumulatorQ32::new();
        for i in 0..16 {
            poly.set_increment(i, 0.05 * (i as f32 + 1.0));
        }
        for _ in 0..20 {
            poly.advance_all();
        }
        let before = poly.phases;
        poly.reset(3);
        assert_eq!(poly.phases[3], 0);
        for (v, (&pp, &pb)) in poly.phases.iter().zip(before.iter()).enumerate() {
            if v != 3 {
                assert_eq!(pp, pb, "voice {v} disturbed by resetting voice 3");
            }
        }
    }

    /// `set_all_increments` followed by `advance_all` keeps every voice in
    /// lockstep.
    #[test]
    fn q32_poly_set_all_increments() {
        let mut poly = PolyPhaseAccumulatorQ32::new();
        poly.set_all_increments(0.01);
        for _ in 0..50 {
            poly.advance_all();
        }
        let first = poly.phases[0];
        assert!(poly.phases.iter().all(|&p| p == first));
        assert!(first > 0);
    }
}
