use crate::approximate::fast_tanh;

/// Single-voice biquad filter kernel (Transposed Direct Form II) with
/// per-sample coefficient interpolation for smooth CV modulation.
pub struct MonoBiquad {
    // Active coefficients (what the filter uses this sample)
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    // Target coefficients (CV interpolation destination)
    b0t: f32,
    b1t: f32,
    b2t: f32,
    a1t: f32,
    a2t: f32,
    // Per-sample deltas
    db0: f32,
    db1: f32,
    db2: f32,
    da1: f32,
    da2: f32,
    // Filter state
    s1: f32,
    s2: f32,
}

impl MonoBiquad {
    /// Create a new `MonoBiquad` with the given initial coefficients.
    ///
    /// Active and target are both set to the provided values; deltas, state,
    /// and the update counter are zeroed.
    pub fn new(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            b0t: b0,
            b1t: b1,
            b2t: b2,
            a1t: a1,
            a2t: a2,
            db0: 0.0,
            db1: 0.0,
            db2: 0.0,
            da1: 0.0,
            da2: 0.0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// Write the given values into both active and target coefficient slots and
    /// zero all deltas. Does not touch filter state or the update counter.
    /// Used when no CV is connected or when parameters change on the static path.
    pub fn set_static(&mut self, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) {
        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
        self.b0t = b0;
        self.b1t = b1;
        self.b2t = b2;
        self.a1t = a1;
        self.a2t = a2;
        self.db0 = 0.0;
        self.db1 = 0.0;
        self.db2 = 0.0;
        self.da1 = 0.0;
        self.da2 = 0.0;
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
    pub fn begin_ramp(
        &mut self,
        b0t: f32,
        b1t: f32,
        b2t: f32,
        a1t: f32,
        a2t: f32,
        interval_recip: f32,
    ) {
        // Snap active to previous targets to eliminate accumulated float drift.
        self.b0 = self.b0t;
        self.b1 = self.b1t;
        self.b2 = self.b2t;
        self.a1 = self.a1t;
        self.a2 = self.a2t;
        // Store new targets.
        self.b0t = b0t;
        self.b1t = b1t;
        self.b2t = b2t;
        self.a1t = a1t;
        self.a2t = a2t;
        // Compute per-sample deltas.
        self.db0 = (b0t - self.b0) * interval_recip;
        self.db1 = (b1t - self.b1) * interval_recip;
        self.db2 = (b2t - self.b2) * interval_recip;
        self.da1 = (a1t - self.a1) * interval_recip;
        self.da2 = (a2t - self.a2) * interval_recip;
    }

    /// Run one sample of the Transposed Direct Form II recurrence, advance
    /// active coefficients by their deltas, and return the output sample.
    pub fn tick(&mut self, x: f32, saturate: bool) -> f32 {
        let y = self.b0 * x + self.s1;
        let fb = if saturate { fast_tanh(y) } else { y };
        self.s1 = self.b1 * x - self.a1 * fb + self.s2;
        self.s2 = self.b2 * x - self.a2 * fb;

        self.b0 += self.db0;
        self.b1 += self.db1;
        self.b2 += self.db2;
        self.a1 += self.da1;
        self.a2 += self.da2;

        y
    }
}

/// 16-voice polyphonic biquad kernel (Transposed Direct Form II) with
/// per-sample coefficient interpolation for smooth CV modulation.
///
/// ## Layout
///
/// All fields are Structure-of-Arrays (`[f32; 16]`), so each field's 16 values
/// are contiguous in memory.  This allows `tick_all` to process all voices in
/// independent per-step loops that LLVM can auto-vectorise with SIMD (e.g. AVX2:
/// 8 × f32 per instruction, two passes over 16 voices).
///
/// ## Cold vs hot
///
/// The five target arrays (`b0t`…`a2t`) are accessed only at update boundaries
/// and sit after the hot fields so they don't pollute the cache lines read every
/// sample.
pub struct PolyBiquad {
    // ── Hot: active coefficients ─────────────────────────────────────────
    pub b0: [f32; 16],
    pub b1: [f32; 16],
    pub b2: [f32; 16],
    pub a1: [f32; 16],
    pub a2: [f32; 16],
    // ── Hot: per-sample coefficient deltas (CV interpolation) ────────────
    pub db0: [f32; 16],
    pub db1: [f32; 16],
    pub db2: [f32; 16],
    pub da1: [f32; 16],
    pub da2: [f32; 16],
    // ── Hot: TDFII filter state ───────────────────────────────────────────
    pub s1: [f32; 16],
    pub s2: [f32; 16],
    // ── Cold: target coefficients (read only at update boundaries) ────────
    pub b0t: [f32; 16],
    pub b1t: [f32; 16],
    pub b2t: [f32; 16],
    pub a1t: [f32; 16],
    pub a2t: [f32; 16],
    /// True when at least one CV input is connected and per-sample coefficient
    /// interpolation is active.  When false all deltas are 0 and the delta
    /// advances in `tick_all` are skipped entirely.
    pub has_cv: bool,
}

impl PolyBiquad {
    /// Create a new `PolyBiquad` with all 16 voices initialised to the given
    /// static coefficients.  All deltas, filter state, and target arrays are
    /// zeroed; `has_cv` is false.
    pub fn new_static(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: [b0; 16],
            b1: [b1; 16],
            b2: [b2; 16],
            a1: [a1; 16],
            a2: [a2; 16],
            db0: [0.0; 16],
            db1: [0.0; 16],
            db2: [0.0; 16],
            da1: [0.0; 16],
            da2: [0.0; 16],
            s1: [0.0; 16],
            s2: [0.0; 16],
            b0t: [b0; 16],
            b1t: [b1; 16],
            b2t: [b2; 16],
            a1t: [a1; 16],
            a2t: [a2; 16],
            has_cv: false,
        }
    }

    /// Fan the given static coefficients into every voice's active and target
    /// fields, and zero all deltas.  Does not touch filter state.
    /// Clears `has_cv` so per-sample delta advances are skipped.
    pub fn set_static(&mut self, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) {
        self.has_cv = false;
        self.b0 = [b0; 16];
        self.b1 = [b1; 16];
        self.b2 = [b2; 16];
        self.a1 = [a1; 16];
        self.a2 = [a2; 16];
        self.db0 = [0.0; 16];
        self.db1 = [0.0; 16];
        self.db2 = [0.0; 16];
        self.da1 = [0.0; 16];
        self.da2 = [0.0; 16];
        self.b0t = [b0; 16];
        self.b1t = [b1; 16];
        self.b2t = [b2; 16];
        self.a1t = [a1; 16];
        self.a2t = [a2; 16];
    }

    /// Snap voice `i`'s active coefficients to its current targets, store the
    /// new targets, and compute per-sample deltas.
    ///
    /// `interval_recip` is `1.0 / periodic_update_interval`, precomputed by the
    /// containing module in `prepare()`.
    /// Call this for each voice at update boundaries after computing per-voice
    /// effective parameters.  Sets `has_cv = true`.
    #[allow(clippy::too_many_arguments)]
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
        // Snap active to the stored targets for this voice.
        self.b0[i] = self.b0t[i];
        self.b1[i] = self.b1t[i];
        self.b2[i] = self.b2t[i];
        self.a1[i] = self.a1t[i];
        self.a2[i] = self.a2t[i];
        // Store new targets.
        self.b0t[i] = b0t;
        self.b1t[i] = b1t;
        self.b2t[i] = b2t;
        self.a1t[i] = a1t;
        self.a2t[i] = a2t;
        // Compute per-sample deltas.
        self.db0[i] = (b0t - self.b0[i]) * interval_recip;
        self.db1[i] = (b1t - self.b1[i]) * interval_recip;
        self.db2[i] = (b2t - self.b2[i]) * interval_recip;
        self.da1[i] = (a1t - self.a1[i]) * interval_recip;
        self.da2[i] = (a2t - self.a2[i]) * interval_recip;
    }

    /// Run one sample of the TDFII recurrence for **all 16 voices** and return
    /// the per-voice output array.
    ///
    /// Each step of the recurrence is a separate loop over 16 independent
    /// elements, enabling auto-vectorisation (e.g. AVX2: 8 × f32 per
    /// instruction, 2 passes per step).
    ///
    /// - `saturate`: when true the feedback path passes through `fast_tanh`.
    ///   This is a loop-invariant branch; LLVM eliminates it at the call site
    ///   when the boolean is a compile-time constant.
    /// - `ramp`: when true (i.e. `has_cv`) the active coefficients are advanced
    ///   by their per-sample deltas after the filter update.  Pass `self.has_cv`.
    pub fn tick_all(&mut self, x: &[f32; 16], saturate: bool, ramp: bool) -> [f32; 16] {
        // Step 1: y = b0*x + s1  — reads b0, x, s1; writes y
        let mut y = [0.0f32; 16];
        for i in 0..16 {
            y[i] = self.b0[i] * x[i] + self.s1[i];
        }

        if saturate {
            // Saturated path: compute fb for all voices first, then update
            // state in separate loops — same split structure as the linear path
            // so LLVM can vectorise each step (vdivps is a valid SIMD op).
            let fb: [f32; 16] = std::array::from_fn(|i| fast_tanh(y[i]));
            // Step 2: new_s1 = b1*x - a1*fb + s2  — reads s2 (old), writes s1
            for i in 0..16 {
                self.s1[i] = self.b1[i] * x[i] - self.a1[i] * fb[i] + self.s2[i];
            }
            // Step 3: new_s2 = b2*x - a2*fb
            for i in 0..16 {
                self.s2[i] = self.b2[i] * x[i] - self.a2[i] * fb[i];
            }
        } else {
            // Linear path: separate loops so LLVM can vectorise each step.
            // Step 2: new_s1 = b1*x - a1*y + s2  — reads s2 (old), writes s1
            for i in 0..16 {
                self.s1[i] = self.b1[i] * x[i] - self.a1[i] * y[i] + self.s2[i];
            }
            // Step 3: new_s2 = b2*x - a2*y  — reads x, y; writes s2
            for i in 0..16 {
                self.s2[i] = self.b2[i] * x[i] - self.a2[i] * y[i];
            }
        }

        // Step 4 (CV path only): advance active coefficients by per-sample deltas
        if ramp {
            for i in 0..16 {
                self.b0[i] += self.db0[i];
                self.b1[i] += self.db1[i];
                self.b2[i] += self.db2[i];
                self.a1[i] += self.da1[i];
                self.a2[i] += self.da2[i];
            }
        }

        y
    }
}

#[cfg(test)]
mod tests;
