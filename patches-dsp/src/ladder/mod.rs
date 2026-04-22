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
use std::f32::consts::PI;

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
    // Active (ramping) coefficients.
    g: f32,
    k: f32,
    drive: f32,
    variant: LadderVariant,
    // Per-sample deltas.
    dg: f32,
    dk: f32,
    dd: f32,
    // Targets (snapped into active at begin_ramp).
    g_target: f32,
    k_target: f32,
    drive_target: f32,
    // Stage state + delayed feedback sample.
    s: [f32; 4],
    y4_prev: f32,
}

impl LadderKernel {
    /// Create a kernel with static (non-ramping) coefficients.
    pub fn new_static(c: LadderCoeffs) -> Self {
        Self {
            g: c.g,
            k: c.k,
            drive: c.drive,
            variant: c.variant,
            dg: 0.0,
            dk: 0.0,
            dd: 0.0,
            g_target: c.g,
            k_target: c.k,
            drive_target: c.drive,
            s: [0.0; 4],
            y4_prev: 0.0,
        }
    }

    /// Snap every coefficient to `c` with no ramp.
    pub fn set_static(&mut self, c: LadderCoeffs) {
        self.g = c.g;
        self.k = c.k;
        self.drive = c.drive;
        self.variant = c.variant;
        self.g_target = c.g;
        self.k_target = c.k;
        self.drive_target = c.drive;
        self.dg = 0.0;
        self.dk = 0.0;
        self.dd = 0.0;
    }

    /// Snap to the previous targets, store new targets, compute deltas.
    ///
    /// `interval_recip = 1.0 / periodic_update_interval`.
    pub fn begin_ramp(&mut self, c: LadderCoeffs, interval_recip: f32) {
        self.g = self.g_target;
        self.k = self.k_target;
        self.drive = self.drive_target;
        self.variant = c.variant;
        self.g_target = c.g;
        self.k_target = c.k;
        self.drive_target = c.drive;
        self.dg = (c.g - self.g) * interval_recip;
        self.dk = (c.k - self.k) * interval_recip;
        self.dd = (c.drive - self.drive) * interval_recip;
    }

    /// Reset stage state to silence without touching coefficients.
    pub fn reset_state(&mut self) {
        self.s = [0.0; 4];
        self.y4_prev = 0.0;
    }

    /// Run one sample through the ladder and return the 4th-stage output.
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let scale = input_scale(self.variant, self.k);
        let u = fast_tanh(self.drive * x * scale - self.k * self.y4_prev);
        let g = self.g;
        let mut y = u;
        for i in 0..4 {
            let v = (y - self.s[i]) * g;
            let yn = v + self.s[i];
            self.s[i] = sanitize(yn + v);
            y = yn;
        }
        self.y4_prev = sanitize(y);
        // Advance ramping coefficients.
        self.g += self.dg;
        self.k += self.dk;
        self.drive += self.dd;
        y
    }
}

// ── PolyLadderKernel (16 voices) ─────────────────────────────────────────────

/// 16-voice polyphonic ladder kernel.
///
/// Variant is shared across all voices (module-level parameter).
/// Each voice has its own cutoff/resonance/drive and stage state.
pub struct PolyLadderKernel {
    g: [f32; 16],
    k: [f32; 16],
    drive: [f32; 16],
    dg: [f32; 16],
    dk: [f32; 16],
    dd: [f32; 16],
    g_target: [f32; 16],
    k_target: [f32; 16],
    drive_target: [f32; 16],
    s0: [f32; 16],
    s1: [f32; 16],
    s2: [f32; 16],
    s3: [f32; 16],
    y4_prev: [f32; 16],
    variant: LadderVariant,
}

impl PolyLadderKernel {
    /// Create with all voices at the same static coefficients.
    pub fn new_static(c: LadderCoeffs) -> Self {
        Self {
            g: [c.g; 16],
            k: [c.k; 16],
            drive: [c.drive; 16],
            dg: [0.0; 16],
            dk: [0.0; 16],
            dd: [0.0; 16],
            g_target: [c.g; 16],
            k_target: [c.k; 16],
            drive_target: [c.drive; 16],
            s0: [0.0; 16],
            s1: [0.0; 16],
            s2: [0.0; 16],
            s3: [0.0; 16],
            y4_prev: [0.0; 16],
            variant: c.variant,
        }
    }

    /// Snap all voices to `c` with no ramp.
    pub fn set_static(&mut self, c: LadderCoeffs) {
        self.g = [c.g; 16];
        self.k = [c.k; 16];
        self.drive = [c.drive; 16];
        self.dg = [0.0; 16];
        self.dk = [0.0; 16];
        self.dd = [0.0; 16];
        self.g_target = [c.g; 16];
        self.k_target = [c.k; 16];
        self.drive_target = [c.drive; 16];
        self.variant = c.variant;
    }

    /// Set the shared voicing variant for every voice.
    pub fn set_variant(&mut self, variant: LadderVariant) {
        self.variant = variant;
    }

    /// Begin a ramp on voice `i` toward `c`.
    pub fn begin_ramp_voice(&mut self, i: usize, c: LadderCoeffs, interval_recip: f32) {
        self.g[i] = self.g_target[i];
        self.k[i] = self.k_target[i];
        self.drive[i] = self.drive_target[i];
        self.g_target[i] = c.g;
        self.k_target[i] = c.k;
        self.drive_target[i] = c.drive;
        self.dg[i] = (c.g - self.g[i]) * interval_recip;
        self.dk[i] = (c.k - self.k[i]) * interval_recip;
        self.dd[i] = (c.drive - self.drive[i]) * interval_recip;
        self.variant = c.variant;
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
        let mut out = [0.0f32; 16];
        for i in 0..16 {
            let scale = input_scale(variant, self.k[i]);
            let u = fast_tanh(self.drive[i] * x[i] * scale - self.k[i] * self.y4_prev[i]);
            let g = self.g[i];

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
            for i in 0..16 {
                self.g[i] += self.dg[i];
                self.k[i] += self.dk[i];
                self.drive[i] += self.dd[i];
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
