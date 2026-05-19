//! Linkwitz-Riley 4th-order filter kernel.
//!
//! Two cascaded Butterworth 2nd-order biquads (each Q = 1/√2) at the
//! same cutoff. Each filter (LP or HP) is −6 dB at the cutoff, so an
//! LR4 LP/HP pair sums to unity magnitude at the crossover (the
//! sum-flat property the [`crate::stereo::mono_bass`] module relies
//! on). Coefficients come from the existing RBJ cookbook helpers in
//! [`crate::filter`]; both stages of a given filter share one
//! coefficient set.

use patches_dsp::MonoBiquad;

/// LR4 filter: two cascaded biquads with shared coefficients.
pub struct Lr4Filter {
    stage1: MonoBiquad,
    stage2: MonoBiquad,
}

impl Lr4Filter {
    /// Create with both stages initialised to the same static coefficients.
    pub fn new(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
        Self {
            stage1: MonoBiquad::new(b0, b1, b2, a1, a2),
            stage2: MonoBiquad::new(b0, b1, b2, a1, a2),
        }
    }

    /// Replace both stages' active and target coefficients; zeroes deltas.
    pub fn set_static(&mut self, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) {
        self.stage1.set_static(b0, b1, b2, a1, a2);
        self.stage2.set_static(b0, b1, b2, a1, a2);
    }

    /// Run one sample through both cascaded stages. Saturation off, no
    /// per-sample coefficient advance — deltas stay zero across the
    /// filter's lifetime since coefficients only change on parameter
    /// updates via [`set_static`].
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y1 = self.stage1.tick_static(x, false);
        self.stage2.tick_static(y1, false)
    }
}
