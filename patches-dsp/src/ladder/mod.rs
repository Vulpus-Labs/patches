//! Zero-delay-feedback 4-pole ladder lowpass kernel.
//!
//! Zavalishin topology (TPT one-pole per stage) with global tanh
//! saturation on the feedback path and a one-sample-delayed feedback
//! term (Huovilainen simplification) so the stages remain explicit.
//!
//! Two coefficient presets model different vintage ladder character:
//!
//! * `Sharp` — unmodified cutoff, unity input scale, sharp resonance peak.
//! * `Smooth` — slight HF loss (stage gain ×0.95) and a resonance-
//!   dependent bass trim on the input (`1 − 0.0875 · k`) for softer top
//!   and bass compression at high resonance.
//!
//! Self-oscillates when `resonance = 1.0` (feedback factor `k = 4`).
//!
//! The kernel ramps its coefficients (`g`, `k`, `drive`) smoothly
//! between periodic updates using deltas computed by `begin_ramp`,
//! matching the pattern used by `svf` and `biquad`.
//!
//! Single-voice: [`LadderKernel`].
//! Poly (16 voices): [`PolyLadderKernel`].

use crate::approximate::fast_tanh;
use crate::coef_ramp::{CoefRamp, CoefTargets, PolyCoefRamp, PolyCoefTargets};
use std::f32::consts::PI;

// Coefficient index order: g=0, k=1, drive=2.
const G: usize = 0;
const K: usize = 1;
const DRIVE: usize = 2;

// ── Variant ──────────────────────────────────────────────────────────────────

/// Coefficient preset for the ladder kernel.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LadderVariant {
    /// Sharp resonance peak, unity input scale, unmodified cutoff.
    Sharp,
    /// Softer top (HF loss) and bass compression under resonance.
    Smooth,
}

#[inline]
fn sanitize(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

#[inline]
fn compute_g(cutoff_hz: f32, sample_rate: f32, variant: LadderVariant) -> f32 {
    // Prewarped one-pole integrator gain. Clamp fc below Nyquist/2 so tan()
    // stays well-behaved even when CV pushes the cutoff up.
    let fc = cutoff_hz.clamp(5.0, sample_rate * 0.45);
    let wd = (PI * fc / sample_rate).tan();
    let g_raw = wd / (1.0 + wd);
    let g = match variant {
        LadderVariant::Sharp => g_raw,
        // Slight HF loss: scale stage gain down a few percent so the ladder's
        // effective cutoff sits a touch below the knob. Audibly softer top.
        LadderVariant::Smooth => g_raw * 0.95,
    };
    // Stage gain is strictly in (0, 1); clamp away from 1.0 for safety.
    g.clamp(1.0e-5, 0.999)
}

#[inline]
fn compute_k(resonance: f32) -> f32 {
    // k = 4.0 puts the ladder on the edge of self-oscillation.
    4.0 * resonance.clamp(0.0, 1.0)
}

#[inline]
fn input_scale(variant: LadderVariant, k: f32) -> f32 {
    match variant {
        LadderVariant::Sharp => 1.0,
        // Bass compression under resonance: trim the input amplitude by up to
        // ~35 % as k sweeps 0 → 4.
        LadderVariant::Smooth => 1.0 - 0.0875 * k,
    }
}

// ── LadderCoeffs ─────────────────────────────────────────────────────────────

/// Frozen ladder coefficients for one instant in time.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LadderCoeffs {
    /// One-pole stage gain `g/(1+g)` in (0, 1).
    pub g: f32,
    /// Global feedback factor in `[0, 4]`. Self-oscillation at 4.
    pub k: f32,
    /// Input drive in `[0, drive_max]` applied before the tanh on the feedback sum.
    pub drive: f32,
    /// Voicing preset (selects input-scale + HF-loss behaviour).
    pub variant: LadderVariant,
}

impl LadderCoeffs {
    /// Compute coefficients from control values.
    #[inline]
    pub fn new(
        cutoff_hz: f32,
        sample_rate: f32,
        resonance: f32,
        drive: f32,
        variant: LadderVariant,
    ) -> Self {
        Self {
            g: compute_g(cutoff_hz, sample_rate, variant),
            k: compute_k(resonance),
            drive: drive.max(0.0),
            variant,
        }
    }
}

// ── LadderKernel (mono) ──────────────────────────────────────────────────────

/// Single-voice ladder kernel with per-sample coefficient interpolation.
pub struct LadderKernel {
    pub coefs: CoefRamp<3>,
    variant: LadderVariant,
    pub targets: CoefTargets<3>,
    // Stage state + delayed feedback sample.
    s: [f32; 4],
    y4_prev: f32,
}

impl LadderKernel {
    /// Create a kernel with static (non-ramping) coefficients.
    pub fn new_static(c: LadderCoeffs) -> Self {
        let values = [c.g, c.k, c.drive];
        Self {
            coefs: CoefRamp::new(values),
            variant: c.variant,
            targets: CoefTargets::new(values),
            s: [0.0; 4],
            y4_prev: 0.0,
        }
    }

    /// Snap every coefficient to `c` with no ramp.
    pub fn set_static(&mut self, c: LadderCoeffs) {
        let values = [c.g, c.k, c.drive];
        self.coefs.set_static(values);
        self.targets.target = values;
        self.variant = c.variant;
    }

    /// Snap to the previous targets, store new targets, compute deltas.
    ///
    /// `interval_recip = 1.0 / periodic_update_interval`.
    #[inline]
    pub fn begin_ramp(&mut self, c: LadderCoeffs, interval_recip: f32) {
        self.variant = c.variant;
        self.coefs
            .begin_ramp([c.g, c.k, c.drive], &mut self.targets, interval_recip);
    }

    /// Reset stage state to silence without touching coefficients.
    pub fn reset_state(&mut self) {
        self.s = [0.0; 4];
        self.y4_prev = 0.0;
    }

    /// Run one sample through the ladder and return the 4th-stage output.
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let g = self.coefs.active[G];
        let k = self.coefs.active[K];
        let drive = self.coefs.active[DRIVE];
        let scale = input_scale(self.variant, k);
        let u = fast_tanh(drive * x * scale - k * self.y4_prev);
        let mut y = u;
        for i in 0..4 {
            let v = (y - self.s[i]) * g;
            let yn = v + self.s[i];
            self.s[i] = sanitize(yn + v);
            y = yn;
        }
        self.y4_prev = sanitize(y);
        self.coefs.advance();
        y
    }
}

// ── PolyLadderKernel (16 voices) ─────────────────────────────────────────────

/// 16-voice polyphonic ladder kernel.
///
/// Variant is shared across all voices (module-level parameter).
/// Each voice has its own cutoff/resonance/drive and stage state.
pub struct PolyLadderKernel {
    pub coefs: PolyCoefRamp<3, 16>,
    s0: [f32; 16],
    s1: [f32; 16],
    s2: [f32; 16],
    s3: [f32; 16],
    y4_prev: [f32; 16],
    pub targets: PolyCoefTargets<3, 16>,
    variant: LadderVariant,
}

impl PolyLadderKernel {
    /// Create with all voices at the same static coefficients.
    pub fn new_static(c: LadderCoeffs) -> Self {
        let values = [c.g, c.k, c.drive];
        Self {
            coefs: PolyCoefRamp::new_static(values),
            s0: [0.0; 16],
            s1: [0.0; 16],
            s2: [0.0; 16],
            s3: [0.0; 16],
            y4_prev: [0.0; 16],
            targets: PolyCoefTargets::new_static(values),
            variant: c.variant,
        }
    }

    /// Snap all voices to `c` with no ramp.
    pub fn set_static(&mut self, c: LadderCoeffs) {
        let values = [c.g, c.k, c.drive];
        self.coefs.set_static(values);
        self.targets.target[G] = [c.g; 16];
        self.targets.target[K] = [c.k; 16];
        self.targets.target[DRIVE] = [c.drive; 16];
        self.variant = c.variant;
    }

    /// Set the shared voicing variant for every voice.
    pub fn set_variant(&mut self, variant: LadderVariant) {
        self.variant = variant;
    }

    /// Begin a ramp on voice `i` toward `c`.
    #[inline]
    pub fn begin_ramp_voice(&mut self, i: usize, c: LadderCoeffs, interval_recip: f32) {
        self.variant = c.variant;
        self.coefs.begin_ramp_voice(
            i,
            [c.g, c.k, c.drive],
            &mut self.targets,
            interval_recip,
        );
    }

    /// Reset integrator state for every voice.
    pub fn reset_state(&mut self) {
        self.s0 = [0.0; 16];
        self.s1 = [0.0; 16];
        self.s2 = [0.0; 16];
        self.s3 = [0.0; 16];
        self.y4_prev = [0.0; 16];
    }

    /// Run one sample for every voice, return the 4th-stage outputs.
    #[inline]
    pub fn tick_all(&mut self, x: &[f32; 16], ramp: bool) -> [f32; 16] {
        let variant = self.variant;
        let g_arr = &self.coefs.active[G];
        let k_arr = &self.coefs.active[K];
        let drive_arr = &self.coefs.active[DRIVE];
        let mut out = [0.0f32; 16];
        for i in 0..16 {
            let k = k_arr[i];
            let scale = input_scale(variant, k);
            let u = fast_tanh(drive_arr[i] * x[i] * scale - k * self.y4_prev[i]);
            let g = g_arr[i];

            let v0 = (u - self.s0[i]) * g;
            let y0 = v0 + self.s0[i];
            self.s0[i] = sanitize(y0 + v0);

            let v1 = (y0 - self.s1[i]) * g;
            let y1 = v1 + self.s1[i];
            self.s1[i] = sanitize(y1 + v1);

            let v2 = (y1 - self.s2[i]) * g;
            let y2 = v2 + self.s2[i];
            self.s2[i] = sanitize(y2 + v2);

            let v3 = (y2 - self.s3[i]) * g;
            let y3 = v3 + self.s3[i];
            self.s3[i] = sanitize(y3 + v3);

            self.y4_prev[i] = sanitize(y3);
            out[i] = y3;
        }
        if ramp {
            self.coefs.advance();
        }
        out
    }
}

#[cfg(test)]
mod tests;
