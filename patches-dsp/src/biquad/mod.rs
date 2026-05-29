use crate::approximate::fast_tanh;
use crate::coef_ramp::{CoefRamp, CoefTargets, PolyCoefRamp, PolyCoefTargets};

// Coefficient index order: b0=0, b1=1, b2=2, a1=3, a2=4.
const B0: usize = 0;
const B1: usize = 1;
const B2: usize = 2;
const A1: usize = 3;
const A2: usize = 4;

/// Single-voice biquad filter kernel (Transposed Direct Form II) with
/// per-sample coefficient interpolation for smooth CV modulation.
pub struct MonoBiquad {
    // Hot: active + per-sample deltas, wrapped in `CoefRamp<5>`.
    pub coefs: CoefRamp<5>,
    // Hot: filter state.
    s1: f32,
    s2: f32,
    // Cold: target coefficients.
    pub targets: CoefTargets<5>,
}

impl MonoBiquad {
    /// Create a new `MonoBiquad` with the given initial coefficients.
    ///
    /// Active and target are both set to the provided values; deltas, state,
    /// and the update counter are zeroed.
    pub fn new(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
        let values = [b0, b1, b2, a1, a2];
        Self {
            coefs: CoefRamp::new(values),
            s1: 0.0,
            s2: 0.0,
            targets: CoefTargets::new(values),
        }
    }

    /// Write the given values into both active and target coefficient slots and
    /// zero all deltas. Does not touch filter state or the update counter.
    /// Used when no CV is connected or when parameters change on the static path.
    pub fn set_static(&mut self, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) {
        let values = [b0, b1, b2, a1, a2];
        self.coefs.set_static(values);
        self.targets.target = values;
    }

    /// Reset the filter delay-line state to zero without disturbing coefficients.
    pub fn reset_state(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    /// Snap active coefficients to the current targets (eliminating accumulated
    /// delta drift), store the new targets, and compute per-sample deltas.
    ///
    /// `interval_recip` is `1.0 / periodic_update_interval`, precomputed by the
    /// containing module in `prepare()` from `AudioEnvironment::periodic_update_interval`.
    /// Call this after computing new target coefficients from live CV values.
    #[inline]
    pub fn begin_ramp(
        &mut self,
        b0t: f32,
        b1t: f32,
        b2t: f32,
        a1t: f32,
        a2t: f32,
        interval_recip: f32,
    ) {
        self.coefs.begin_ramp(
            [b0t, b1t, b2t, a1t, a2t],
            &mut self.targets,
            interval_recip,
        );
    }

    /// Run one sample of the Transposed Direct Form II recurrence, advance
    /// active coefficients by their deltas, and return the output sample.
    pub fn tick(&mut self, x: f32, saturate: bool) -> f32 {
        let c = &self.coefs.active;
        let y = c[B0] * x + self.s1;
        let fb = if saturate { fast_tanh(y) } else { y };
        self.s1 = c[B1] * x - c[A1] * fb + self.s2;
        self.s2 = c[B2] * x - c[A2] * fb;

        self.coefs.advance();

        y
    }

    /// Like [`tick`] but skips the per-sample coefficient `advance()`.
    ///
    /// Use after [`set_static`] when no CV is driving coefficients (deltas
    /// are zero); saves five adds per sample. Calling this after
    /// [`begin_ramp`] would freeze the coefficient ramp.
    #[inline]
    pub fn tick_static(&mut self, x: f32, saturate: bool) -> f32 {
        let c = &self.coefs.active;
        let y = c[B0] * x + self.s1;
        let fb = if saturate { fast_tanh(y) } else { y };
        self.s1 = c[B1] * x - c[A1] * fb + self.s2;
        self.s2 = c[B2] * x - c[A2] * fb;
        y
    }
}

/// N-voice polyphonic biquad kernel (Transposed Direct Form II) with
/// per-sample coefficient interpolation for smooth CV modulation.
///
/// ## Layout
///
/// All fields are Structure-of-Arrays (`[f32; N]`), so each field's N values
/// are contiguous in memory.  This allows `tick_all` to process all voices in
/// independent per-step loops that LLVM can auto-vectorise with SIMD (e.g. AVX2:
/// 8 × f32 per instruction, two passes over 16 voices; one pass for N=8).
///
/// ## Cold vs hot
///
/// The five target arrays (inside `CoefTargets`) are accessed only at update
/// boundaries and sit after the hot fields so they don't pollute the cache
/// lines read every sample.
pub struct BiquadN<const N: usize> {
    // ── Hot: active coefficients + per-sample deltas ─────────────────────
    pub coefs: PolyCoefRamp<5, N>,
    // ── Hot: TDFII filter state ───────────────────────────────────────────
    pub s1: [f32; N],
    pub s2: [f32; N],
    // ── Cold: target coefficients (read only at update boundaries) ────────
    pub targets: PolyCoefTargets<5, N>,
    /// True when at least one voice has a CV-driven ramp active. When false
    /// all deltas are 0 and the delta advances in `tick_all` are skipped.
    pub has_cv: bool,
}

/// Backwards-compatible 16-voice alias.
pub type PolyBiquad = BiquadN<16>;

impl<const N: usize> BiquadN<N> {
    /// Create a new biquad with every voice initialised to the given static
    /// coefficients. All deltas, filter state, and target arrays are zeroed;
    /// `has_cv` is false.
    pub fn new_static(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
        let values = [b0, b1, b2, a1, a2];
        Self {
            coefs: PolyCoefRamp::new_static(values),
            s1: [0.0; N],
            s2: [0.0; N],
            targets: PolyCoefTargets::new_static(values),
            has_cv: false,
        }
    }

    /// Fan the given static coefficients into every voice's active and target
    /// fields, and zero all deltas.  Does not touch filter state.
    /// Clears `has_cv` so per-sample delta advances are skipped.
    pub fn set_static(&mut self, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) {
        self.has_cv = false;
        let values = [b0, b1, b2, a1, a2];
        self.coefs.set_static(values);
        for (slot, v) in self.targets.target.iter_mut().zip(values.iter()) {
            *slot = [*v; N];
        }
    }

    /// Install per-voice static coefficients into one slot. Does not change
    /// the `has_cv` flag — call `clear_has_cv()` when no voice is ramping.
    /// Use when each voice carries different fixed coefficients (e.g. the
    /// FDN's per-line absorption filters).
    #[inline]
    pub fn set_static_voice(&mut self, i: usize, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) {
        let values = [b0, b1, b2, a1, a2];
        self.coefs.set_static_voice(i, values);
        for (slot, v) in self.targets.target.iter_mut().zip(values.iter()) {
            slot[i] = *v;
        }
    }

    /// Clear the `has_cv` flag — skips per-sample delta advances in `tick_all`.
    #[inline]
    pub fn clear_has_cv(&mut self) {
        self.has_cv = false;
    }

    /// Snap voice `i`'s active coefficients to its current targets, store the
    /// new targets, and compute per-sample deltas.
    ///
    /// `interval_recip` is `1.0 / periodic_update_interval`, precomputed by the
    /// containing module in `prepare()`.
    /// Call this for each voice at update boundaries after computing per-voice
    /// effective parameters.  Sets `has_cv = true`.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn begin_ramp_voice(
        &mut self,
        i: usize,
        b0t: f32,
        b1t: f32,
        b2t: f32,
        a1t: f32,
        a2t: f32,
        interval_recip: f32,
    ) {
        self.has_cv = true;
        self.coefs.begin_ramp_voice(
            i,
            [b0t, b1t, b2t, a1t, a2t],
            &mut self.targets,
            interval_recip,
        );
    }

    /// Run one sample of the TDFII recurrence for **all N voices** and return
    /// the per-voice output array.
    ///
    /// Each step of the recurrence is a separate loop over N independent
    /// elements, enabling auto-vectorisation.
    ///
    /// - `saturate`: when true the feedback path passes through `fast_tanh`.
    ///   Loop-invariant; LLVM eliminates the branch at the call site when
    ///   the boolean is a compile-time constant.
    /// - `ramp`: when true (i.e. `has_cv`) the active coefficients are advanced
    ///   by their per-sample deltas after the filter update.  Pass `self.has_cv`.
    pub fn tick_all(&mut self, x: &[f32; N], saturate: bool, ramp: bool) -> [f32; N] {
        let b0 = &self.coefs.active[B0];
        let b1 = &self.coefs.active[B1];
        let b2 = &self.coefs.active[B2];
        let a1 = &self.coefs.active[A1];
        let a2 = &self.coefs.active[A2];

        let mut y = [0.0f32; N];
        for i in 0..N {
            y[i] = b0[i] * x[i] + self.s1[i];
        }

        if saturate {
            let fb: [f32; N] = std::array::from_fn(|i| fast_tanh(y[i]));
            for i in 0..N {
                self.s1[i] = b1[i] * x[i] - a1[i] * fb[i] + self.s2[i];
            }
            for i in 0..N {
                self.s2[i] = b2[i] * x[i] - a2[i] * fb[i];
            }
        } else {
            for i in 0..N {
                self.s1[i] = b1[i] * x[i] - a1[i] * y[i] + self.s2[i];
            }
            for i in 0..N {
                self.s2[i] = b2[i] * x[i] - a2[i] * y[i];
            }
        }

        if ramp {
            self.coefs.advance();
        }

        y
    }
}

#[cfg(test)]
mod tests;
