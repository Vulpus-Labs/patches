//! R3109/IR3109-style OTA ladder lowpass kernel.
//!
//! Four TPT one-pole stages like [`crate::ladder`], but the nonlinearity
//! lives **inside each integrator** rather than on the global feedback
//! sum. This matches the softer, more distributed saturation of
//! OTA-C filter chips (IR3109, CEM3320, etc.) and produces a cleaner,
//! more sinusoidal self-oscillation than a Moog-style transistor ladder.
//!
//! Differences from [`crate::ladder::LadderKernel`]:
//!
//! * Per-stage `tanh` on the integrator input — no global pre-feedback tanh.
//! * No resonance-dependent input attenuation (Juno-style filters do not
//!   thin the bass under high resonance).
//! * Selectable 2-pole (12 dB/oct) or 4-pole (24 dB/oct) output tap.
//!   The resonance feedback loop always comes from the 4th stage so
//!   the filter self-oscillates identically at `k ≈ 4` in either mode;
//!   the 2-pole tap simply reads stage 2's output and inherits the
//!   resonance peak shaped by the full 4-pole loop.
//! * `k` is passed in raw engineering units; callers scale
//!   `resonance ∈ [0, 1]` by [`OtaPoles::k_max`] (always `4.0` here —
//!   retained for symmetry with pole-dependent scaling in other designs).
//!
//! Self-oscillation is supported in both modes. Drift is **not** handled
//! here — callers fold any cutoff modulation (including the engine
//! `GLOBAL_DRIFT` backplane slot) into the `cutoff_hz` they pass to
//! [`OtaLadderCoeffs::new`].
//!
//! Single-voice: [`OtaLadderKernel`].
//! Poly (16 voices): [`PolyOtaLadderKernel`].

use crate::approximate::fast_tanh;
use crate::coef_ramp::{CoefRamp, CoefTargets, PolyCoefRamp, PolyCoefTargets};
use std::f32::consts::PI;

// Coefficient index order: g=0, k=1, drive=2.
const G: usize = 0;
const K: usize = 1;
const DRIVE: usize = 2;

// ── Poles ────────────────────────────────────────────────────────────────────

/// Filter order (output tap + feedback tap).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OtaPoles {
    /// 12 dB/oct: feedback and output from stage 1.
    Two,
    /// 24 dB/oct: feedback and output from stage 3.
    Four,
}

impl OtaPoles {
    #[inline]
    fn output_tap(self) -> usize {
        match self {
            OtaPoles::Two => 1,
            OtaPoles::Four => 3,
        }
    }

    /// Feedback factor at which the filter sits on the edge of
    /// self-oscillation. Same for both modes because feedback is always
    /// taken from stage 3.
    #[inline]
    pub fn k_max(self) -> f32 {
        4.0
    }
}

#[inline]
fn sanitize(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

#[inline]
fn compute_g(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let fc = cutoff_hz.clamp(5.0, sample_rate * 0.45);
    let wd = (PI * fc / sample_rate).tan();
    let g = wd / (1.0 + wd);
    g.clamp(1.0e-5, 0.999)
}

// ── OtaLadderCoeffs ──────────────────────────────────────────────────────────

/// Frozen OTA-ladder coefficients for one instant in time.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct OtaLadderCoeffs {
    /// TPT one-pole stage gain in (0, 1).
    pub g: f32,
    /// Feedback factor in engineering units. Self-osc at `poles.k_max()`.
    pub k: f32,
    /// Input drive applied before stage 0's tanh.
    pub drive: f32,
}

impl OtaLadderCoeffs {
    /// Compute coefficients from control values. `k` is raw — caller scales
    /// `resonance ∈ [0, 1]` by [`OtaPoles::k_max`].
    #[inline]
    pub fn new(cutoff_hz: f32, sample_rate: f32, k: f32, drive: f32) -> Self {
        Self {
            g: compute_g(cutoff_hz, sample_rate),
            k: k.max(0.0),
            drive: drive.max(0.0),
        }
    }
}

// ── OtaLadderKernel (mono) ───────────────────────────────────────────────────

/// Single-voice OTA-ladder kernel with per-sample coefficient interpolation.
pub struct OtaLadderKernel {
    pub coefs: CoefRamp<3>,
    poles: OtaPoles,
    pub targets: CoefTargets<3>,
    s: [f32; 4],
    y4_prev: f32,
}

impl OtaLadderKernel {
    /// Create a kernel with static (non-ramping) coefficients.
    pub fn new_static(c: OtaLadderCoeffs, poles: OtaPoles) -> Self {
        let values = [c.g, c.k, c.drive];
        Self {
            coefs: CoefRamp::new(values),
            poles,
            targets: CoefTargets::new(values),
            s: [0.0; 4],
            y4_prev: 0.0,
        }
    }

    /// Snap every coefficient to `c` with no ramp.
    pub fn set_static(&mut self, c: OtaLadderCoeffs) {
        let values = [c.g, c.k, c.drive];
        self.coefs.set_static(values);
        self.targets.target = values;
    }

    /// Change output-tap mode. Feedback path is unchanged — the filter
    /// continues to ring identically; only the slope of the output tap
    /// shifts between 12 and 24 dB/oct.
    pub fn set_poles(&mut self, poles: OtaPoles) {
        self.poles = poles;
    }

    /// Current pole mode.
    pub fn poles(&self) -> OtaPoles {
        self.poles
    }

    /// Snap to the previous targets, store new targets, compute deltas.
    #[inline]
    pub fn begin_ramp(&mut self, c: OtaLadderCoeffs, interval_recip: f32) {
        self.coefs
            .begin_ramp([c.g, c.k, c.drive], &mut self.targets, interval_recip);
    }

    /// Reset stage state to silence without touching coefficients.
    pub fn reset_state(&mut self) {
        self.s = [0.0; 4];
        self.y4_prev = 0.0;
    }

    /// Run one sample through the ladder and return the selected output tap.
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let g = self.coefs.active[G];
        let k = self.coefs.active[K];
        let drive = self.coefs.active[DRIVE];
        let fed = drive * x - k * self.y4_prev;
        let mut input = fed;
        let mut stages = [0.0f32; 4];
        for (i, stage) in stages.iter_mut().enumerate() {
            let u = fast_tanh(input);
            let v = (u - self.s[i]) * g;
            let yn = v + self.s[i];
            self.s[i] = sanitize(yn + v);
            *stage = yn;
            input = yn;
        }
        self.y4_prev = sanitize(stages[3]);
        self.coefs.advance();
        stages[self.poles.output_tap()]
    }
}

// ── PolyOtaLadderKernel (16 voices) ──────────────────────────────────────────

/// 16-voice polyphonic OTA ladder kernel.
///
/// `poles` is shared across all voices (module-level parameter).
/// Each voice carries its own cutoff/resonance/drive and stage state.
pub struct PolyOtaLadderKernel {
    pub coefs: PolyCoefRamp<3, 16>,
    s0: [f32; 16],
    s1: [f32; 16],
    s2: [f32; 16],
    s3: [f32; 16],
    y4_prev: [f32; 16],
    pub targets: PolyCoefTargets<3, 16>,
    poles: OtaPoles,
}

impl PolyOtaLadderKernel {
    pub fn new_static(c: OtaLadderCoeffs, poles: OtaPoles) -> Self {
        let values = [c.g, c.k, c.drive];
        Self {
            coefs: PolyCoefRamp::new_static(values),
            s0: [0.0; 16],
            s1: [0.0; 16],
            s2: [0.0; 16],
            s3: [0.0; 16],
            y4_prev: [0.0; 16],
            targets: PolyCoefTargets::new_static(values),
            poles,
        }
    }

    pub fn set_static(&mut self, c: OtaLadderCoeffs) {
        let values = [c.g, c.k, c.drive];
        self.coefs.set_static(values);
        self.targets.target[G] = [c.g; 16];
        self.targets.target[K] = [c.k; 16];
        self.targets.target[DRIVE] = [c.drive; 16];
    }

    pub fn set_poles(&mut self, poles: OtaPoles) {
        self.poles = poles;
    }

    pub fn poles(&self) -> OtaPoles {
        self.poles
    }

    #[inline]
    pub fn begin_ramp_voice(&mut self, i: usize, c: OtaLadderCoeffs, interval_recip: f32) {
        self.coefs.begin_ramp_voice(
            i,
            [c.g, c.k, c.drive],
            &mut self.targets,
            interval_recip,
        );
    }

    pub fn reset_state(&mut self) {
        self.s0 = [0.0; 16];
        self.s1 = [0.0; 16];
        self.s2 = [0.0; 16];
        self.s3 = [0.0; 16];
        self.y4_prev = [0.0; 16];
    }

    #[inline]
    pub fn tick_all(&mut self, x: &[f32; 16], ramp: bool) -> [f32; 16] {
        let tap = self.poles.output_tap();
        let g_arr = &self.coefs.active[G];
        let k_arr = &self.coefs.active[K];
        let drive_arr = &self.coefs.active[DRIVE];
        let mut out = [0.0f32; 16];
        for i in 0..16 {
            let g = g_arr[i];
            let fed = drive_arr[i] * x[i] - k_arr[i] * self.y4_prev[i];

            let u0 = fast_tanh(fed);
            let v0 = (u0 - self.s0[i]) * g;
            let y0 = v0 + self.s0[i];
            self.s0[i] = sanitize(y0 + v0);

            let u1 = fast_tanh(y0);
            let v1 = (u1 - self.s1[i]) * g;
            let y1 = v1 + self.s1[i];
            self.s1[i] = sanitize(y1 + v1);

            let u2 = fast_tanh(y1);
            let v2 = (u2 - self.s2[i]) * g;
            let y2 = v2 + self.s2[i];
            self.s2[i] = sanitize(y2 + v2);

            let u3 = fast_tanh(y2);
            let v3 = (u3 - self.s3[i]) * g;
            let y3 = v3 + self.s3[i];
            self.s3[i] = sanitize(y3 + v3);

            self.y4_prev[i] = sanitize(y3);
            out[i] = [y0, y1, y2, y3][tap];
        }
        if ramp {
            self.coefs.advance();
        }
        out
    }
}

#[cfg(test)]
mod tests;
