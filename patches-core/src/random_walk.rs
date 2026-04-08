/// Per-update step size used for the engine-level global drift walk.
///
/// At 44 100 Hz this gives a bandwidth of roughly 0.5–1 Hz and keeps the
/// stationary range within ±1. Used by the audio callback, which advances
/// the walk every sample.
pub const GLOBAL_DRIFT_STEP: f32 = 0.0002;

/// Per-update step size for oscillator-internal drift walks.
///
/// Oscillator drift walks are advanced every 64 samples rather than every
/// sample, so the per-advance step is larger to produce a similar perceived
/// drift rate (≈ 0.5–2 Hz wander).
pub const OSCILLATOR_DRIFT_STEP: f32 = 0.005;

/// Half a semitone expressed in V/OCT units (1/24 of an octave).
///
/// Used to scale a `[-1, 1]` walk value so that `drift = 1.0` produces at
/// most ±half a semitone of pitch deviation.
pub const HALF_SEMITONE_VOCT: f32 = 1.0 / 24.0;

/// A bounded random walk driven by a 32-bit linear congruential generator.
///
/// Each call to [`advance`] adds `step * noise` to the current value (where
/// `noise ∈ [-1.0, 1.0]`) and clamps the result to `[-1.0, 1.0]`.
///
/// The walk is entirely deterministic: the same `seed` always produces the
/// same sequence. Derive per-instance seeds from [`InstanceId::as_u64`] with
/// a non-zero offset so that different module instances drift independently.
///
/// [`advance`]: BoundedRandomWalk::advance
/// [`InstanceId::as_u64`]: crate::InstanceId::as_u64
pub struct BoundedRandomWalk {
    rng: u32,
    value: f32,
    step: f32,
}

impl BoundedRandomWalk {
    /// Create a new walk with the given `seed` and `step` size.
    pub fn new(seed: u32, step: f32) -> Self {
        Self { rng: seed, value: 0.0, step }
    }

    /// Advance the LCG, update the bounded value, and return it.
    #[inline(always)]
    pub fn advance(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (self.rng as i32 as f32) * (1.0 / 2_147_483_648.0);
        self.value = (self.value + noise * self.step).clamp(-1.0, 1.0);
        self.value
    }

    /// Return the current value without advancing the walk.
    pub fn value(&self) -> f32 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_value_is_zero() {
        assert_eq!(BoundedRandomWalk::new(0x1234_5678, 0.1).value(), 0.0);
    }

    #[test]
    fn value_stays_in_bounds() {
        let mut walk = BoundedRandomWalk::new(0xDEAD_BEEF, 1.0); // large step
        for _ in 0..10_000 {
            let v = walk.advance();
            assert!((-1.0..=1.0).contains(&v), "value {v} out of bounds");
        }
    }

    #[test]
    fn same_seed_produces_same_sequence() {
        let mut a = BoundedRandomWalk::new(42, 0.01);
        let mut b = BoundedRandomWalk::new(42, 0.01);
        for _ in 0..100 {
            assert_eq!(a.advance(), b.advance());
        }
    }

    #[test]
    fn different_seeds_produce_different_sequences() {
        let mut a = BoundedRandomWalk::new(1, 0.01);
        let mut b = BoundedRandomWalk::new(2, 0.01);
        let different = (0..100).any(|_| a.advance() != b.advance());
        assert!(different, "different seeds should diverge");
    }
}
