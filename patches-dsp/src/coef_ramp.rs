//! Coefficient-ramp primitives for filter kernels.
//!
//! See [ADR 0050](../../../adr/0050-coef-ramp-primitive.md) for the
//! design rationale (hot/cold split, no `remaining` counter, how
//! kernel-specific invariants like SVF's `stability_clamp` sit on
//! top of the generic primitive).
//!
//! Captures the snap-on-begin / store-target / compute-delta /
//! per-sample-advance pattern duplicated across biquad, SVF, ladder, and
//! OTA ladder kernels. Two structs per arity:
//!
//! - [`CoefRamp<K>`] holds the hot fields — `active` and `delta` — that are
//!   read/written every sample by the filter's recurrence and advance step.
//! - [`CoefTargets<K>`] holds the cold `target` field, read only at update
//!   boundaries.
//!
//! Kernels place `CoefRamp` in their hot region (alongside state) and
//! `CoefTargets` in their cold region (after state) so that the cache-line
//! layout that `tick_all` relies on is preserved. Nothing here forces a
//! particular struct layout — the kernel keeps control.
//!
//! ## No `remaining` counter
//!
//! Drift is handled by snapping `active ← previous target` at the *start*
//! of the next `begin_ramp` call, matching the existing kernels. There is
//! no per-sample "span finished" check — the ramp keeps advancing until
//! the next update, and the next `begin_ramp` re-bases off the stored
//! target (not the drifted active).
//!
//! ## Poly shape
//!
//! `PolyCoefRamp<K, N>` stores `[[f32; N]; K]` — K coefficients, each a
//! contiguous `[f32; N]` voice array. `advance` is a nested loop: the
//! inner `for i in 0..N` over the fixed-size voice array is what
//! autovectorises (AVX2: two passes of 8 × f32 for N=16); the outer
//! `for k in 0..K` unrolls at compile time.

/// Scalar coefficient ramp — hot fields (active + delta).
#[derive(Debug, Clone, Copy)]
pub struct CoefRamp<const K: usize> {
    pub active: [f32; K],
    pub delta: [f32; K],
}

/// Scalar coefficient ramp — cold field (target).
#[derive(Debug, Clone, Copy)]
pub struct CoefTargets<const K: usize> {
    pub target: [f32; K],
}

impl<const K: usize> CoefRamp<K> {
    #[inline]
    pub const fn new(values: [f32; K]) -> Self {
        Self { active: values, delta: [0.0; K] }
    }

    /// Snap active to `values`, zero deltas. Caller should also set
    /// `CoefTargets::target = values` to keep snap-on-begin coherent.
    #[inline]
    pub fn set_static(&mut self, values: [f32; K]) {
        self.active = values;
        self.delta = [0.0; K];
    }

    /// Snap active ← previous targets (eliminating drift), store new
    /// targets, compute per-sample deltas.
    #[inline]
    pub fn begin_ramp(
        &mut self,
        new_targets: [f32; K],
        targets: &mut CoefTargets<K>,
        interval_recip: f32,
    ) {
        self.active = targets.target;
        targets.target = new_targets;
        for (d, (t, a)) in self.delta.iter_mut().zip(new_targets.iter().zip(self.active.iter())) {
            *d = (*t - *a) * interval_recip;
        }
    }

    /// `active[k] += delta[k]` for all k. Call once per sample after the
    /// kernel's recurrence step.
    #[inline]
    pub fn advance(&mut self) {
        for k in 0..K {
            self.active[k] += self.delta[k];
        }
    }
}

impl<const K: usize> CoefTargets<K> {
    #[inline]
    pub const fn new(values: [f32; K]) -> Self {
        Self { target: values }
    }
}

/// Poly coefficient ramp — hot fields (active + delta), SoA per coef.
#[derive(Debug, Clone, Copy)]
pub struct PolyCoefRamp<const K: usize, const N: usize> {
    pub active: [[f32; N]; K],
    pub delta: [[f32; N]; K],
}

/// Poly coefficient ramp — cold field (target), SoA per coef.
#[derive(Debug, Clone, Copy)]
pub struct PolyCoefTargets<const K: usize, const N: usize> {
    pub target: [[f32; N]; K],
}

impl<const K: usize, const N: usize> PolyCoefRamp<K, N> {
    pub fn new_static(values: [f32; K]) -> Self {
        let mut active = [[0.0; N]; K];
        for (slot, v) in active.iter_mut().zip(values.iter()) {
            *slot = [*v; N];
        }
        Self { active, delta: [[0.0; N]; K] }
    }

    /// Broadcast `values` across all voices, zero all deltas.
    pub fn set_static(&mut self, values: [f32; K]) {
        for (slot, v) in self.active.iter_mut().zip(values.iter()) {
            *slot = [*v; N];
        }
        for slot in self.delta.iter_mut() {
            *slot = [0.0; N];
        }
    }

    /// Snap voice `i` active ← previous targets, store new per-voice
    /// targets, compute deltas.
    #[inline]
    pub fn begin_ramp_voice(
        &mut self,
        i: usize,
        new_targets: [f32; K],
        targets: &mut PolyCoefTargets<K, N>,
        interval_recip: f32,
    ) {
        for ((active, delta), (tgt_slot, new_t)) in self
            .active
            .iter_mut()
            .zip(self.delta.iter_mut())
            .zip(targets.target.iter_mut().zip(new_targets.iter()))
        {
            active[i] = tgt_slot[i];
            tgt_slot[i] = *new_t;
            delta[i] = (*new_t - active[i]) * interval_recip;
        }
    }

    /// Per-sample advance across all voices. Inner loop over N is what
    /// autovectorises; outer K unrolls.
    #[inline]
    pub fn advance(&mut self) {
        for k in 0..K {
            for i in 0..N {
                self.active[k][i] += self.delta[k][i];
            }
        }
    }
}

impl<const K: usize, const N: usize> PolyCoefTargets<K, N> {
    pub fn new_static(values: [f32; K]) -> Self {
        let mut target = [[0.0; N]; K];
        for (slot, v) in target.iter_mut().zip(values.iter()) {
            *slot = [*v; N];
        }
        Self { target }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_begin_ramp_snaps_and_computes_delta() {
        let mut r = CoefRamp::<3>::new([0.0; 3]);
        let mut t = CoefTargets::<3>::new([0.0; 3]);
        // First ramp: active starts at 0, target 0 → new target 1. Delta = 1 / 8.
        r.begin_ramp([1.0, 2.0, -1.0], &mut t, 1.0 / 8.0);
        assert_eq!(r.active, [0.0, 0.0, 0.0]);
        assert_eq!(t.target, [1.0, 2.0, -1.0]);
        assert!((r.delta[0] - 0.125).abs() < 1e-6);
        assert!((r.delta[1] - 0.25).abs() < 1e-6);
        assert!((r.delta[2] - (-0.125)).abs() < 1e-6);
    }

    #[test]
    fn scalar_advance_approaches_target() {
        let mut r = CoefRamp::<2>::new([0.0; 2]);
        let mut t = CoefTargets::<2>::new([0.0; 2]);
        r.begin_ramp([1.0, 2.0], &mut t, 1.0 / 16.0);
        for _ in 0..16 {
            r.advance();
        }
        assert!((r.active[0] - 1.0).abs() < 1e-4);
        assert!((r.active[1] - 2.0).abs() < 1e-4);
    }

    #[test]
    fn scalar_second_ramp_snaps_to_previous_target() {
        let mut r = CoefRamp::<1>::new([0.0]);
        let mut t = CoefTargets::<1>::new([0.0]);
        r.begin_ramp([1.0], &mut t, 1.0 / 8.0);
        // Advance only partway — active drifts.
        for _ in 0..4 { r.advance(); }
        let before = r.active[0];
        assert!(before > 0.4 && before < 0.6);
        // New ramp: active must snap to previous target (1.0), not stay drifted.
        r.begin_ramp([2.0], &mut t, 1.0 / 8.0);
        assert_eq!(r.active[0], 1.0);
        assert_eq!(t.target[0], 2.0);
        assert!((r.delta[0] - 0.125).abs() < 1e-6);
    }

    #[test]
    fn scalar_set_static_zeroes_delta() {
        let mut r = CoefRamp::<2>::new([0.0; 2]);
        let mut t = CoefTargets::<2>::new([0.0; 2]);
        r.begin_ramp([1.0, 2.0], &mut t, 1.0 / 8.0);
        assert_ne!(r.delta, [0.0; 2]);
        r.set_static([5.0, 6.0]);
        assert_eq!(r.active, [5.0, 6.0]);
        assert_eq!(r.delta, [0.0; 2]);
    }

    #[test]
    fn poly_begin_ramp_voice_independent() {
        let mut r = PolyCoefRamp::<2, 4>::new_static([0.0, 0.0]);
        let mut t = PolyCoefTargets::<2, 4>::new_static([0.0, 0.0]);
        r.begin_ramp_voice(0, [1.0, 0.0], &mut t, 1.0 / 8.0);
        r.begin_ramp_voice(2, [0.0, 2.0], &mut t, 1.0 / 8.0);
        assert_eq!(t.target[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(t.target[1], [0.0, 0.0, 2.0, 0.0]);
        assert!((r.delta[0][0] - 0.125).abs() < 1e-6);
        assert_eq!(r.delta[0][1], 0.0);
        assert_eq!(r.delta[0][2], 0.0);
        assert!((r.delta[1][2] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn poly_advance_approaches_target() {
        let mut r = PolyCoefRamp::<3, 16>::new_static([0.0, 0.0, 0.0]);
        let mut t = PolyCoefTargets::<3, 16>::new_static([0.0, 0.0, 0.0]);
        for i in 0..16 {
            r.begin_ramp_voice(i, [i as f32, -(i as f32), 0.5], &mut t, 1.0 / 32.0);
        }
        for _ in 0..32 {
            r.advance();
        }
        for i in 0..16 {
            assert!((r.active[0][i] - i as f32).abs() < 1e-3);
            assert!((r.active[1][i] - -(i as f32)).abs() < 1e-3);
            assert!((r.active[2][i] - 0.5).abs() < 1e-4);
        }
    }

    #[test]
    fn poly_second_ramp_snaps_to_stored_target() {
        let mut r = PolyCoefRamp::<1, 4>::new_static([0.0]);
        let mut t = PolyCoefTargets::<1, 4>::new_static([0.0]);
        r.begin_ramp_voice(1, [1.0], &mut t, 1.0 / 8.0);
        for _ in 0..4 { r.advance(); }
        assert!(r.active[0][1] > 0.4 && r.active[0][1] < 0.6);
        r.begin_ramp_voice(1, [2.0], &mut t, 1.0 / 8.0);
        assert_eq!(r.active[0][1], 1.0);
        assert_eq!(t.target[0][1], 2.0);
    }

    #[test]
    fn poly_set_static_zeroes_all_deltas() {
        let mut r = PolyCoefRamp::<2, 4>::new_static([0.0, 0.0]);
        let mut t = PolyCoefTargets::<2, 4>::new_static([0.0, 0.0]);
        r.begin_ramp_voice(0, [1.0, 2.0], &mut t, 1.0 / 8.0);
        r.set_static([5.0, 6.0]);
        for k in 0..2 {
            for i in 0..4 {
                assert_eq!(r.delta[k][i], 0.0);
            }
        }
        assert_eq!(r.active[0], [5.0; 4]);
        assert_eq!(r.active[1], [6.0; 4]);
    }
}
