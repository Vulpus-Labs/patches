//! Fixed-point detuned saw ensemble — the DSP kernel behind the `HyperSaw`
//! and `PolyHyperSaw` modules (ADR 0078).
//!
//! A "supersaw": [`N_COPIES`] detuned sawtooth copies (1 centre + 8 sides)
//! summed per voice, run across a fixed [`N_VOICES`]-wide batch. The slow
//! inter-copy beating gives the wide, animated lead/pad character.
//!
//! # Why this is not the f32 `polyblep` path
//!
//! 144 saws (9 copies × 16 voices) cannot recompute frequency, an `exp2`, and a
//! reciprocal per sample. So all of that — detune ratios, base increments,
//! reciprocals, density/mix gains — is computed by the *caller* at control rate
//! and pushed in via [`HyperSawCore::update`]. The core owns only phase
//! accumulation and the PolyBLEP-corrected weighted sum. It calls no
//! transcendentals and does not know the sample rate.
//!
//! # Layout and vectorisation
//!
//! State is **copy-major** (`[copy][voice]`) so the per-sample loop runs the
//! voice dimension (16 = 2× `u32x8`) in the inner position — the axis the
//! autovectoriser can pack. The phase accumulator is fixed-point `u32` (exact,
//! branch-free wrap at 2³²); the PolyBLEP residual is computed in **f32** to
//! dodge the u32×u32→hi32 multiply that x86 AVX2 has no instruction for and that
//! autovectorisers refuse to lower (ADR 0078 §2). Phase, increment and wrap
//! detection stay u32 (plain adds/compares, which vectorise); everything else is
//! f32.
//!
//! # Numeric contract with the caller
//!
//! - `inc[k][v]` — Q32 per-sample phase increment (frequency · 2³² / sample_rate
//!   for copy `k`, voice `v`). `0` is safe (DC / unconnected): the copy emits a
//!   constant and no PolyBLEP correction.
//! - `inv_inc[k][v]` — **reciprocal of the increment**, `1.0 / inc as f32`, used
//!   to normalise the PolyBLEP fraction into `[0, 1)`. The caller must set this
//!   to `0.0` when `inc == 0` to avoid a division by zero on its side; the core
//!   simply multiplies by it.
//! - `gain[k][v]` — per-copy/voice amplitude weight. Folds density fade-in, the
//!   centre/side mix, and loudness normalisation. The core sums `gain · saw`
//!   across copies with no further scaling, so the caller is responsible for
//!   keeping the summed output in `[-1, 1]`.

/// Saw copies per voice: 1 centre + 8 sides (4 below + 4 above the centre).
pub const N_COPIES: usize = 9;
/// Fixed voice-batch width. Mono (`HyperSaw`) runs the full batch and reads
/// lane 0; the wasted lanes are accepted for one vectorised kernel (ADR 0078).
pub const N_VOICES: usize = 16;
/// Detuned side pairs (one below + one above per pair).
pub const N_PAIRS: usize = 4;

/// Q31 → `[-1, 1)` scale: `2^-31`.
const SAW_SCALE: f32 = 1.0 / 2_147_483_648.0;

/// Voice-batched 9-saw ensemble. See the module docs for the layout and the
/// numeric contract.
pub struct HyperSawCore {
    /// Fixed-point phase per copy/voice; wraps free at 2³².
    phase: [[u32; N_VOICES]; N_COPIES],
    /// Q32 per-sample phase increment. Copy 0 = centre; 1..=8 = sides.
    inc: [[u32; N_VOICES]; N_COPIES],
    /// Reciprocal `1/inc` as f32 — the PolyBLEP fraction normaliser.
    inv_inc: [[f32; N_VOICES]; N_COPIES],
    /// Per-copy/voice amplitude weight (density + mix + loudness norm).
    gain: [[f32; N_VOICES]; N_COPIES],
}

impl HyperSawCore {
    /// Construct with every copy of every voice at a distinct, deterministic
    /// start phase drawn from `xorshift64(seed)`.
    ///
    /// The detune is what gives the width, but aligned start phases would give a
    /// thin attack and coincident wraps (comb-filtered aliasing + high crest
    /// factor → headroom loss). Decorrelating once at construction avoids this;
    /// the detuned increments then keep the copies drifting apart on their own.
    /// There is no re-trigger (free-running oscillator), so no re-randomisation.
    ///
    /// Increments, reciprocals and gains start at zero — the core emits silence
    /// until the caller's first [`update`](Self::update).
    pub fn new(seed: u64) -> Self {
        // xorshift64 requires a non-zero state; map 0 to a fixed odd constant.
        let mut state = if seed == 0 { 0xDEAD_BEEF_CAFE_F00D } else { seed };
        let mut phase = [[0u32; N_VOICES]; N_COPIES];
        for copy in phase.iter_mut() {
            for slot in copy.iter_mut() {
                // Advance the generator and take the live state bits as phase;
                // the returned f32 is unused here.
                let _ = crate::noise::xorshift64(&mut state);
                *slot = (state >> 32) as u32;
            }
        }
        Self {
            phase,
            inc: [[0; N_VOICES]; N_COPIES],
            inv_inc: [[0.0; N_VOICES]; N_COPIES],
            gain: [[0.0; N_VOICES]; N_COPIES],
        }
    }

    /// Push control-rate state: increments, their reciprocals, and gains.
    ///
    /// No allocation, no transcendentals — a plain copy. See the module docs
    /// for the meaning and the `inc == 0` / `inv_inc == 0` convention.
    pub fn update(
        &mut self,
        inc: &[[u32; N_VOICES]; N_COPIES],
        inv_inc: &[[f32; N_VOICES]; N_COPIES],
        gain: &[[f32; N_VOICES]; N_COPIES],
    ) {
        self.inc = *inc;
        self.inv_inc = *inv_inc;
        self.gain = *gain;
    }

    /// The Q32 phase increment for `copy`/`voice` (read-only introspection;
    /// callers use it to verify the detune ratios they pushed).
    pub fn increment(&self, copy: usize, voice: usize) -> u32 {
        self.inc[copy][voice]
    }

    /// Advance one sample, writing the per-voice summed output.
    ///
    /// The inner loop runs the 16-voice batch with a fixed trip count and no
    /// early exit; the body is f32 arithmetic over the lanes except the phase
    /// accumulate/wrap (u32 add + compare). Both are shapes the autovectoriser
    /// packs into NEON / AVX2 — verified by the ASM gate in ticket 0958.
    pub fn process(&mut self, out: &mut [f32; N_VOICES]) {
        *out = [0.0; N_VOICES];
        for copy in 0..N_COPIES {
            let phase = &mut self.phase[copy];
            let inc = &self.inc[copy];
            let inv_inc = &self.inv_inc[copy];
            let gain = &self.gain[copy];
            for v in 0..N_VOICES {
                let inc_v = inc[v];
                let p = phase[v].wrapping_add(inc_v);
                phase[v] = p;

                // Naive saw: phase reinterpreted as signed Q31, scaled to [-1, 1).
                let naive = (p.wrapping_sub(0x8000_0000) as i32) as f32 * SAW_SCALE;

                // Only the wrap discontinuity aliases; correct it once per cycle.
                // Two mutually-exclusive zones, so at most one correction per
                // wrap per copy (the 0955/0956 double-emit failure is excluded by
                // construction). `before` is masked by `!after` so the exact
                // p == 0 landing counts only as `after`.
                let after = p < inc_v; // just wrapped: phase small
                let before = !after & (p.wrapping_neg() < inc_v); // about to wrap

                // local = distance to the wrap (p after, 2^32 − p before).
                let local = if after { p } else { p.wrapping_neg() } as f32;
                let frac = local * inv_inc[v]; // ∈ [0, 1); inv_inc is 1/inc
                let one_minus = 1.0 - frac;
                let sq = one_minus * one_minus;

                // residual: −sq after the wrap, +sq before it, 0 elsewhere.
                let sign = (before as u32 as f32) - (after as u32 as f32);
                let saw = naive - sign * sq;

                out[v] += gain[v] * saw;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fft::RealPackedFft;

    /// Drive a single centre copy at increment `inc_u` and collect `n` samples.
    fn single_saw_inc(core: &mut HyperSawCore, inc_u: u32, n: usize) -> Vec<f32> {
        let mut inc = [[0u32; N_VOICES]; N_COPIES];
        let mut inv = [[0.0f32; N_VOICES]; N_COPIES];
        let mut gain = [[0.0f32; N_VOICES]; N_COPIES];
        inc[0][0] = inc_u;
        inv[0][0] = 1.0 / inc_u as f32;
        gain[0][0] = 1.0;
        core.update(&inc, &inv, &gain);

        let mut out = [0.0f32; N_VOICES];
        (0..n)
            .map(|_| {
                core.process(&mut out);
                out[0]
            })
            .collect()
    }

    /// Drive a single centre copy at `freq`/`sr` and collect `n` samples.
    fn single_saw(core: &mut HyperSawCore, freq: f32, sr: f32, n: usize) -> Vec<f32> {
        single_saw_inc(core, (freq / sr * 4_294_967_296.0) as u32, n)
    }

    #[test]
    fn new_is_deterministic_and_decorrelated() {
        let a = HyperSawCore::new(0x1234_5678);
        let b = HyperSawCore::new(0x1234_5678);
        assert_eq!(a.phase, b.phase, "same seed must be bit-identical");

        // All start phases distinct (no aligned copies/voices).
        let mut all: Vec<u32> = a.phase.iter().flatten().copied().collect();
        let total = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), total, "start phases must be distinct");

        // A different seed gives a different layout.
        let c = HyperSawCore::new(0x9999_9999);
        assert_ne!(a.phase, c.phase);
    }

    #[test]
    fn zero_seed_is_safe() {
        let z = HyperSawCore::new(0);
        // Non-zero state means non-degenerate phases.
        let distinct: std::collections::HashSet<u32> =
            z.phase.iter().flatten().copied().collect();
        assert!(distinct.len() > 1, "zero seed must still decorrelate");
    }

    #[test]
    fn silent_until_updated() {
        let mut core = HyperSawCore::new(42);
        let mut out = [0.0f32; N_VOICES];
        for _ in 0..64 {
            core.process(&mut out);
            assert_eq!(out, [0.0; N_VOICES], "no gain → silence");
        }
    }

    #[test]
    fn dc_increment_is_constant_and_finite() {
        // inc = 0 must emit a finite constant with no PolyBLEP, no NaN/Inf.
        let mut core = HyperSawCore::new(7);
        let mut gain = [[0.0f32; N_VOICES]; N_COPIES];
        gain[0][0] = 1.0;
        core.update(
            &[[0u32; N_VOICES]; N_COPIES],
            &[[0.0f32; N_VOICES]; N_COPIES],
            &gain,
        );
        let mut out = [0.0f32; N_VOICES];
        core.process(&mut out);
        let first = out[0];
        assert!(first.is_finite());
        for _ in 0..16 {
            core.process(&mut out);
            assert_eq!(out[0], first, "DC copy must hold a constant");
        }
    }

    /// Sum the magnitude of every bin below Nyquist that is *not* a true
    /// harmonic of fundamental bin `k` — i.e. the aliasing + folding floor.
    /// A bandlimited saw has energy only on multiples of `k`; folded harmonics
    /// land elsewhere, so this isolates alias energy.
    fn alias_energy(signal: &[f32], n: usize, k: usize) -> f32 {
        let fft = RealPackedFft::new(n);
        let mut buf = vec![0.0f32; n];
        buf.copy_from_slice(signal);
        fft.forward(&mut buf);
        let half = n / 2;
        (1..half)
            .filter(|bin| bin % k != 0) // exclude true harmonics
            .map(|bin| crate::test_support::bin_magnitude(&buf, bin))
            .sum()
    }

    #[test]
    fn polyblep_suppresses_aliasing() {
        // Bin-aligned frequency: k cycles in exactly n samples → no FFT leakage,
        // and (n a power of two) the u32 increment is exact, so the frame is
        // perfectly periodic. True harmonics fall on multiples of k; folded
        // (aliased) harmonics land on other bins. PolyBLEP must cut that alias
        // floor sharply vs the naive saw — a wrong-sign residual would *raise*
        // it, so this also guards polarity.
        let n = 8192; // 2^13
        let k = 467; // prime; harmonics 1..8 below Nyquist, rest alias
        let inc_u = (k * (1 << 19)) as u32; // k / n * 2^32, exact

        let mut core = HyperSawCore::new(1);
        let blep = single_saw_inc(&mut core, inc_u, n);

        // Naive saw at the same increment, no correction.
        let mut phase = 0u32;
        let naive: Vec<f32> = (0..n)
            .map(|_| {
                phase = phase.wrapping_add(inc_u);
                (phase.wrapping_sub(0x8000_0000) as i32) as f32 * SAW_SCALE
            })
            .collect();

        let blep_alias = alias_energy(&blep, n, k);
        let naive_alias = alias_energy(&naive, n, k);
        assert!(
            blep_alias < naive_alias * 0.5,
            "polyBLEP should cut alias floor by >6 dB: blep {blep_alias:.3} vs naive {naive_alias:.3}"
        );
    }

    #[test]
    fn detuned_copies_beat_and_widen() {
        // Three copies slightly detuned → slow amplitude beating; peak envelope
        // must vary more than a single steady saw.
        let sr = 48_000.0;
        let base = 220.0;
        let n = 48_000; // 1 s — long enough for sub-Hz beats.

        let to_inc = |f: f32| (f / sr * 4_294_967_296.0) as u32;
        let freqs = [base, base * 1.003, base * 0.997];
        let mut inc = [[0u32; N_VOICES]; N_COPIES];
        let mut inv = [[0.0f32; N_VOICES]; N_COPIES];
        let mut gain = [[0.0f32; N_VOICES]; N_COPIES];
        for (k, &f) in freqs.iter().enumerate() {
            let i = to_inc(f);
            inc[k][0] = i;
            inv[k][0] = 1.0 / i as f32;
            gain[k][0] = 1.0 / freqs.len() as f32;
        }
        let mut core = HyperSawCore::new(99);
        core.update(&inc, &inv, &gain);

        let mut out = [0.0f32; N_VOICES];
        let detuned: Vec<f32> = (0..n)
            .map(|_| {
                core.process(&mut out);
                out[0]
            })
            .collect();

        // Single saw, same total gain, for comparison.
        let mut core1 = HyperSawCore::new(99);
        let single = single_saw(&mut core1, base, sr, n);

        let env_var = |s: &[f32]| {
            // Peak-per-block envelope variance.
            let block = 1024;
            let peaks: Vec<f32> = s
                .chunks(block)
                .map(|c| c.iter().fold(0.0f32, |m, &x| m.max(x.abs())))
                .collect();
            let mean = peaks.iter().sum::<f32>() / peaks.len() as f32;
            peaks.iter().map(|&p| (p - mean) * (p - mean)).sum::<f32>() / peaks.len() as f32
        };

        assert!(
            env_var(&detuned) > env_var(&single) * 4.0,
            "detuned ensemble should beat far more than a single saw"
        );
        assert!(detuned.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn weighted_sum_stays_bounded() {
        // Full 9-copy stack, loudness-normalised gains, must stay in [-1, 1].
        let sr = 44_100.0;
        let base = 110.0;
        let to_inc = |f: f32| (f / sr * 4_294_967_296.0) as u32;

        let mut inc = [[0u32; N_VOICES]; N_COPIES];
        let mut inv = [[0.0f32; N_VOICES]; N_COPIES];
        let mut gain = [[0.0f32; N_VOICES]; N_COPIES];
        // Spread copies over ±50 cents; normalise by copy count.
        let g = 1.0 / N_COPIES as f32;
        for k in 0..N_COPIES {
            let cents = (k as f32 - 4.0) * 12.5; // -50..+50
            let f = base * 2.0f32.powf(cents / 1200.0);
            for v in 0..N_VOICES {
                let i = to_inc(f);
                inc[k][v] = i;
                inv[k][v] = 1.0 / i as f32;
                gain[k][v] = g;
            }
        }
        let mut core = HyperSawCore::new(5);
        core.update(&inc, &inv, &gain);

        let mut out = [0.0f32; N_VOICES];
        let mut peak = 0.0f32;
        for _ in 0..sr as usize {
            core.process(&mut out);
            for &x in out.iter() {
                assert!(x.is_finite(), "no NaN/Inf");
                peak = peak.max(x.abs());
            }
        }
        assert!(peak <= 1.0, "loudness-normalised stack exceeded [-1,1]: {peak}");
    }

    #[test]
    fn fractional_density_pair_equivalence() {
        // The modules sum one half-weighted boundary pair instead of weighting
        // every side. Validate the two are equal to within rounding: a pair at
        // gain `g` on copies {a, b} equals copies {a, b} weighted g each.
        let sr = 48_000.0;
        let to_inc = |f: f32| (f / sr * 4_294_967_296.0) as u32;
        let (fa, fb) = (330.0f32, 333.0f32);
        let g = 0.37f32;

        // Build a core with copies 0 and 1 at gain g.
        let build = |gain_a: f32, gain_b: f32| {
            let mut inc = [[0u32; N_VOICES]; N_COPIES];
            let mut inv = [[0.0f32; N_VOICES]; N_COPIES];
            let mut gain = [[0.0f32; N_VOICES]; N_COPIES];
            for (k, (&f, &gg)) in [fa, fb].iter().zip([gain_a, gain_b].iter()).enumerate() {
                let i = to_inc(f);
                inc[k][0] = i;
                inv[k][0] = 1.0 / i as f32;
                gain[k][0] = gg;
            }
            let mut core = HyperSawCore::new(3);
            core.update(&inc, &inv, &gain);
            core
        };

        let mut paired = build(g, g);
        // Reference: two independent single saws scaled by g and summed.
        let mut ref_a = build(g, 0.0);
        let mut ref_b = build(0.0, g);

        let mut o = [0.0f32; N_VOICES];
        for i in 0..256 {
            paired.process(&mut o);
            let got = o[0];
            ref_a.process(&mut o);
            let a = o[0];
            ref_b.process(&mut o);
            let b = o[0];
            assert!(
                (got - (a + b)).abs() < 1e-5,
                "pair sum mismatch at {i}: {got} vs {}",
                a + b
            );
        }
    }
}
