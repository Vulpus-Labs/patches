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
use std::f32::consts::PI;

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
    g: f32,
    k: f32,
    drive: f32,
    dg: f32,
    dk: f32,
    dd: f32,
    g_target: f32,
    k_target: f32,
    drive_target: f32,
    poles: OtaPoles,
    s: [f32; 4],
    y4_prev: f32,
}

impl OtaLadderKernel {
    /// Create a kernel with static (non-ramping) coefficients.
    pub fn new_static(c: OtaLadderCoeffs, poles: OtaPoles) -> Self {
        Self {
            g: c.g,
            k: c.k,
            drive: c.drive,
            dg: 0.0,
            dk: 0.0,
            dd: 0.0,
            g_target: c.g,
            k_target: c.k,
            drive_target: c.drive,
            poles,
            s: [0.0; 4],
            y4_prev: 0.0,
        }
    }

    /// Snap every coefficient to `c` with no ramp.
    pub fn set_static(&mut self, c: OtaLadderCoeffs) {
        self.g = c.g;
        self.k = c.k;
        self.drive = c.drive;
        self.g_target = c.g;
        self.k_target = c.k;
        self.drive_target = c.drive;
        self.dg = 0.0;
        self.dk = 0.0;
        self.dd = 0.0;
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
    pub fn begin_ramp(&mut self, c: OtaLadderCoeffs, interval_recip: f32) {
        self.g = self.g_target;
        self.k = self.k_target;
        self.drive = self.drive_target;
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

    /// Run one sample through the ladder and return the selected output tap.
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let g = self.g;
        let fed = self.drive * x - self.k * self.y4_prev;
        let mut input = fed;
        let mut stages = [0.0f32; 4];
        for i in 0..4 {
            let u = fast_tanh(input);
            let v = (u - self.s[i]) * g;
            let yn = v + self.s[i];
            self.s[i] = sanitize(yn + v);
            stages[i] = yn;
            input = yn;
        }
        self.y4_prev = sanitize(stages[3]);
        self.g += self.dg;
        self.k += self.dk;
        self.drive += self.dd;
        stages[self.poles.output_tap()]
    }
}

// ── PolyOtaLadderKernel (16 voices) ──────────────────────────────────────────

/// 16-voice polyphonic OTA ladder kernel.
///
/// `poles` is shared across all voices (module-level parameter).
/// Each voice carries its own cutoff/resonance/drive and stage state.
pub struct PolyOtaLadderKernel {
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
    poles: OtaPoles,
}

impl PolyOtaLadderKernel {
    pub fn new_static(c: OtaLadderCoeffs, poles: OtaPoles) -> Self {
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
            poles,
        }
    }

    pub fn set_static(&mut self, c: OtaLadderCoeffs) {
        self.g = [c.g; 16];
        self.k = [c.k; 16];
        self.drive = [c.drive; 16];
        self.dg = [0.0; 16];
        self.dk = [0.0; 16];
        self.dd = [0.0; 16];
        self.g_target = [c.g; 16];
        self.k_target = [c.k; 16];
        self.drive_target = [c.drive; 16];
    }

    pub fn set_poles(&mut self, poles: OtaPoles) {
        self.poles = poles;
    }

    pub fn poles(&self) -> OtaPoles {
        self.poles
    }

    pub fn begin_ramp_voice(&mut self, i: usize, c: OtaLadderCoeffs, interval_recip: f32) {
        self.g[i] = self.g_target[i];
        self.k[i] = self.k_target[i];
        self.drive[i] = self.drive_target[i];
        self.g_target[i] = c.g;
        self.k_target[i] = c.k;
        self.drive_target[i] = c.drive;
        self.dg[i] = (c.g - self.g[i]) * interval_recip;
        self.dk[i] = (c.k - self.k[i]) * interval_recip;
        self.dd[i] = (c.drive - self.drive[i]) * interval_recip;
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
        let mut out = [0.0f32; 16];
        for i in 0..16 {
            let g = self.g[i];
            let fed = self.drive[i] * x[i] - self.k[i] * self.y4_prev[i];

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
