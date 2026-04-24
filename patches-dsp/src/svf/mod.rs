//! Chamberlin State-Variable Filter (SVF) kernel.
//!
//! This module provides `SvfKernel` (single-voice) and `PolySvfKernel` (16-voice)
//! that encapsulate the coefficient computation and per-sample state update for the
//! Chamberlin SVF topology:
//!
//! ```text
//! hp = input - lp - q_damp * bp
//! bp = bp + f * hp
//! lp = lp + f * bp
//! ```
//!
//! Both types support coefficient ramping: each `begin_ramp` call snaps active
//! coefficients to the previous targets, stores new targets, and computes
//! per-sample deltas so that `tick` / `tick_all` interpolate smoothly to the
//! new values over the next update interval.

use std::f32::consts::PI;

use crate::coef_ramp::{CoefRamp, CoefTargets, PolyCoefRamp, PolyCoefTargets};

// Coefficient index order: f=0, q=1.
const F: usize = 0;
const Q: usize = 1;

// ── Coefficient helpers ──────────────────────────────────────────────────────

/// Compute the Chamberlin SVF frequency coefficient from cutoff Hz.
///
/// `f = 2 · sin(π · fc / fs)`, with `fc` clamped to `[1, 0.499 · fs]` to stay
/// within the region where the topology is numerically stable.
#[inline]
pub fn svf_f(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let fc = cutoff_hz.clamp(1.0, sample_rate * 0.499);
    2.0 * (PI * fc / sample_rate).sin()
}

/// Maps normalised Q [0, 1] to SVF damping coefficient using an exponential curve.
///
/// | q   | damping | Q (approx) | character                        |
/// |-----|---------|------------|----------------------------------|
/// | 0.0 | 2.0     | 0.5        | maximally damped, no peak        |
/// | 0.5 | 0.14    | 7          | moderate resonance               |
/// | 0.9 | 0.014   | 70         | very high resonance              |
/// | 1.0 | 0.01    | 100        | self-oscillating                 |
#[inline]
pub fn q_to_damp(q: f32) -> f32 {
    2.0 * (0.005_f32).powf(q)
}

/// Replace NaN / ±Inf with 0.0 so a single bad sample cannot permanently
/// corrupt the integrator state.
#[inline]
fn sanitize(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// Clamp `f` so the Chamberlin SVF remains stable for damping `d`.
///
/// The topology's characteristic polynomial is `λ² − (2−f²−fd)λ + (1−fd)`.
/// Both roots stay inside the unit circle iff `f² + 2fd < 4`.  Solving for `f`
/// gives `f < (−d + √(d² + 16)) / 2`.  We subtract a small margin (0.05) so
/// the filter never operates right at the stability boundary, where transient
/// gains are extreme enough to overflow `f32`.
#[inline]
pub fn stability_clamp(f: f32, d: f32) -> f32 {
    let f_max = 0.5 * (-d + (d * d + 16.0).sqrt()) - 0.05;
    f.min(f_max)
}

// ── SvfCoeffs ────────────────────────────────────────────────────────────────

/// Frozen Chamberlin SVF coefficients: frequency sweep `f` and damping `q_damp`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SvfCoeffs {
    /// Frequency coefficient: `f = 2 · sin(π · fc / fs)`.
    pub f: f32,
    /// Damping coefficient: `q_damp = 2 · 0.005^q`.
    pub q_damp: f32,
}

impl SvfCoeffs {
    /// Compute coefficients from cutoff (Hz), sample rate (Hz), and normalised Q [0, 1].
    #[inline]
    pub fn new(cutoff_hz: f32, sample_rate: f32, q_norm: f32) -> Self {
        Self {
            f: svf_f(cutoff_hz, sample_rate),
            q_damp: q_to_damp(q_norm),
        }
    }
}

// ── SvfState ─────────────────────────────────────────────────────────────────

/// Integrator state for one SVF voice.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SvfState {
    /// Lowpass integrator state.
    pub lp: f32,
    /// Bandpass integrator state.
    pub bp: f32,
}

impl SvfState {
    /// Reset state to zero (silence).
    #[inline]
    pub fn reset(&mut self) {
        self.lp = 0.0;
        self.bp = 0.0;
    }
}

// ── SvfKernel ────────────────────────────────────────────────────────────────

/// Single-voice Chamberlin SVF kernel with per-sample coefficient interpolation.
///
/// Holds all hot state (active coefficients, deltas, filter memory) so that the
/// containing module's `process()` body is free of bookkeeping.
pub struct SvfKernel {
    pub coefs: CoefRamp<2>,
    pub state: SvfState,
    pub targets: CoefTargets<2>,
}

impl SvfKernel {
    /// Create a new kernel with static (non-ramping) coefficients.
    pub fn new_static(f: f32, d: f32) -> Self {
        let f = stability_clamp(f, d);
        let values = [f, d];
        Self {
            coefs: CoefRamp::new(values),
            state: SvfState::default(),
            targets: CoefTargets::new(values),
        }
    }

    /// Create from an `SvfCoeffs` value.
    #[inline]
    pub fn from_coeffs(c: SvfCoeffs) -> Self {
        Self::new_static(c.f, c.q_damp)
    }

    /// Immediately snap all coefficients to `f` / `d` with no ramp.
    pub fn set_static(&mut self, f: f32, d: f32) {
        let f = stability_clamp(f, d);
        let values = [f, d];
        self.coefs.set_static(values);
        self.targets.target = values;
    }

    /// Snap active coefficients to the previous targets, store new targets,
    /// and compute per-sample deltas for the next update window.
    ///
    /// Applies `stability_clamp` both to the new target and to the snapped
    /// active `f` (using previous targets) — this is the kernel-specific
    /// twist on the generic snap-on-begin.
    ///
    /// `interval_recip` is `1.0 / periodic_update_interval`, precomputed by
    /// the caller to avoid a division in the audio path.
    #[inline]
    pub fn begin_ramp(&mut self, ft: f32, dt: f32, interval_recip: f32) {
        let ft = stability_clamp(ft, dt);
        let prev_f = self.targets.target[F];
        let prev_q = self.targets.target[Q];
        self.coefs.active[F] = stability_clamp(prev_f, prev_q);
        self.coefs.active[Q] = prev_q;
        self.targets.target = [ft, dt];
        self.coefs.delta[F] = (ft - self.coefs.active[F]) * interval_recip;
        self.coefs.delta[Q] = (dt - self.coefs.active[Q]) * interval_recip;
    }

    /// Reset integrator state to zero without touching coefficients.
    pub fn reset_state(&mut self) {
        self.state.reset();
    }

    /// Run one Chamberlin SVF sample, advance interpolating coefficients,
    /// and return `(lp, hp, bp)`.
    #[inline]
    pub fn tick(&mut self, x: f32) -> (f32, f32, f32) {
        let f = self.coefs.active[F];
        let q = self.coefs.active[Q];
        let lp = self.state.lp + f * self.state.bp;
        let hp = x - lp - q * self.state.bp;
        let bp = self.state.bp + f * hp;
        self.state.lp = sanitize(lp);
        self.state.bp = sanitize(bp);
        self.coefs.advance();
        (lp, hp, bp)
    }
}

// ── PolySvfKernel ─────────────────────────────────────────────────────────────

/// 16-voice polyphonic SVF kernel (Chamberlin topology).
///
/// All fields are Structure-of-Arrays (`[f32; 16]`), so each field's 16 values
/// are contiguous in memory.  `tick_all` processes all voices together in
/// independent per-step loops, enabling auto-vectorisation (AVX2: 8 × f32 per
/// instruction, 2 passes per step).
///
/// Cold target arrays sit after the hot fields to avoid polluting the cache
/// lines read every sample.
pub struct PolySvfKernel {
    // ── Hot: active coefficients + per-sample deltas ─────────────────────
    pub coefs: PolyCoefRamp<2, 16>,
    // ── Hot: integrator state ─────────────────────────────────────────────
    pub lp_state: [f32; 16],
    pub bp_state: [f32; 16],
    // ── Cold: targets (read only at update boundaries) ────────────────────
    pub targets: PolyCoefTargets<2, 16>,
}

impl PolySvfKernel {
    /// Create a new kernel with all 16 voices set to the same static coefficients.
    pub fn new_static(f: f32, d: f32) -> Self {
        let f = stability_clamp(f, d);
        let values = [f, d];
        Self {
            coefs: PolyCoefRamp::new_static(values),
            lp_state: [0.0; 16],
            bp_state: [0.0; 16],
            targets: PolyCoefTargets::new_static(values),
        }
    }

    /// Create from an `SvfCoeffs` value, broadcasting to all 16 voices.
    #[inline]
    pub fn from_coeffs(c: SvfCoeffs) -> Self {
        Self::new_static(c.f, c.q_damp)
    }

    /// Immediately set all 16 voices to the same static coefficients, no ramp.
    pub fn set_static(&mut self, f: f32, d: f32) {
        let f = stability_clamp(f, d);
        let values = [f, d];
        self.coefs.set_static(values);
        self.targets.target[F] = [f; 16];
        self.targets.target[Q] = [d; 16];
    }

    /// Snap voice `i` to its stored targets, store new targets, compute deltas.
    /// Applies `stability_clamp` to both the new target `f` and the snapped
    /// active `f` (using previous targets).
    ///
    /// `interval_recip` is `1.0 / periodic_update_interval`, precomputed by
    /// the caller.
    #[inline]
    pub fn begin_ramp_voice(&mut self, i: usize, ft: f32, dt: f32, interval_recip: f32) {
        let ft = stability_clamp(ft, dt);
        let prev_f = self.targets.target[F][i];
        let prev_q = self.targets.target[Q][i];
        self.coefs.active[F][i] = stability_clamp(prev_f, prev_q);
        self.coefs.active[Q][i] = prev_q;
        self.targets.target[F][i] = ft;
        self.targets.target[Q][i] = dt;
        self.coefs.delta[F][i] = (ft - self.coefs.active[F][i]) * interval_recip;
        self.coefs.delta[Q][i] = (dt - self.coefs.active[Q][i]) * interval_recip;
    }

    /// Reset all 16 voices' integrator state to zero without touching coefficients.
    pub fn reset_state(&mut self) {
        self.lp_state = [0.0; 16];
        self.bp_state = [0.0; 16];
    }

    /// Run one Chamberlin SVF sample for **all 16 voices** and return
    /// `(lp, hp, bp)` output arrays.
    ///
    /// Each step of the recurrence is a separate loop over 16 independent
    /// elements, enabling auto-vectorisation.  When `ramp` is false the
    /// coefficient-advance loop is skipped entirely.
    #[inline]
    pub fn tick_all(
        &mut self,
        x: &[f32; 16],
        ramp: bool,
    ) -> ([f32; 16], [f32; 16], [f32; 16]) {
        let f = &self.coefs.active[F];
        let q = &self.coefs.active[Q];
        // Step 1: lp = lp_state + f_coeff * bp_state  — independent across voices
        let lp: [f32; 16] =
            std::array::from_fn(|i| self.lp_state[i] + f[i] * self.bp_state[i]);
        // Step 2: hp = x - lp - q_damp * bp_state  — depends on lp[], not lp[i±1]
        let hp: [f32; 16] =
            std::array::from_fn(|i| x[i] - lp[i] - q[i] * self.bp_state[i]);
        // Step 3: bp = bp_state + f_coeff * hp
        let bp: [f32; 16] =
            std::array::from_fn(|i| self.bp_state[i] + f[i] * hp[i]);
        // State update (sanitize to prevent NaN/Inf propagation)
        self.lp_state = std::array::from_fn(|i| sanitize(lp[i]));
        self.bp_state = std::array::from_fn(|i| sanitize(bp[i]));
        // Step 4 (CV path only): advance active coefficients
        if ramp {
            self.coefs.advance();
        }
        (lp, hp, bp)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────


#[cfg(test)]
mod tests;
