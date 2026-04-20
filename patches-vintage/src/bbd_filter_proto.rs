//! Sub-sample-evaluated continuous-time filter prototype.
//!
//! A single complex one-pole section `dx/dt = p·x + u(t)` with `u(t)`
//! piecewise-constant between host samples. State is advanced once
//! per host sample using the closed-form solution of the ODE; the
//! filter can also be evaluated at any sub-sample fraction `τ ∈ [0,1]`
//! without re-running anything — that's the piece the full H-P-style
//! BBD needs in order to sample its input at BBD clock moments that
//! don't align with host samples.
//!
//! Convention:
//! - `u[n]` is the input value held over `[n·Ts, (n+1)·Ts)`.
//! - `x[n] = y(n·Ts)` is the state at the start of sample `n`.
//! - `advance(u)` takes `u[n]` and rolls `x[n]` forward to `x[n+1]`.
//! - Before `advance`, `evaluate(τ, u)` gives `y(n·Ts + τ·Ts)`.
//!
//! Closed-form: with `φ(τ) = exp(p·τ·Ts)` and `ψ(τ) = (φ(τ)-1)/p`,
//! `evaluate(τ, u) = φ(τ)·x + ψ(τ)·u` and `advance(u)` is exactly
//! `evaluate(1, u)`. Both share the same formula so the stitch
//! between samples is analytical, not approximate.
//!
//! This module is a prototype — not wired into [`crate::bbd`] yet.

// Minimal complex-f32 helper — avoids pulling `num-complex` for one file.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex32 {
    pub re: f32,
    pub im: f32,
}

impl Complex32 {
    pub const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }
    pub fn conj(self) -> Self {
        Self { re: self.re, im: -self.im }
    }
    pub fn exp(self) -> Self {
        let m = self.re.exp();
        let (s, c) = self.im.sin_cos();
        Self { re: m * c, im: m * s }
    }
    pub fn powf(self, b: f32) -> Self {
        let mag = (self.re * self.re + self.im * self.im).sqrt();
        let ang = self.im.atan2(self.re);
        let new_mag = mag.powf(b);
        let (s, c) = (ang * b).sin_cos();
        Self { re: new_mag * c, im: new_mag * s }
    }
}

impl std::ops::Add for Complex32 {
    type Output = Self;
    fn add(self, o: Self) -> Self { Self { re: self.re + o.re, im: self.im + o.im } }
}
impl std::ops::Sub for Complex32 {
    type Output = Self;
    fn sub(self, o: Self) -> Self { Self { re: self.re - o.re, im: self.im - o.im } }
}
impl std::ops::Mul for Complex32 {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
}
impl std::ops::Mul<f32> for Complex32 {
    type Output = Self;
    fn mul(self, s: f32) -> Self { Self { re: self.re * s, im: self.im * s } }
}
impl std::ops::Div for Complex32 {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        let d = o.re * o.re + o.im * o.im;
        Self {
            re: (self.re * o.re + self.im * o.im) / d,
            im: (self.im * o.re - self.re * o.im) / d,
        }
    }
}
impl std::ops::Neg for Complex32 {
    type Output = Self;
    fn neg(self) -> Self { Self { re: -self.re, im: -self.im } }
}
impl std::ops::AddAssign for Complex32 {
    fn add_assign(&mut self, o: Self) { self.re += o.re; self.im += o.im; }
}

/// One continuous-time complex pole with closed-form sub-sample
/// evaluation. Real output is `re(evaluate(…))` in the bank sum.
#[derive(Clone, Debug)]
pub struct ContinuousPole {
    /// Continuous-time pole (rad/s). Typically Re(p) < 0 for stability.
    pole: Complex32,
    host_ts: f32,
    /// `φ(1) = exp(p·Ts)` — per-host-sample state transition.
    pole_corr: Complex32,
    /// `ψ(1) = (φ(1) - 1) / p` — per-host-sample input response.
    psi1: Complex32,
    /// State `x[n] = y(n·Ts)`.
    x: Complex32,
}

impl ContinuousPole {
    pub fn new(pole: Complex32, sample_rate: f32) -> Self {
        let host_ts = 1.0 / sample_rate;
        let pole_corr = (pole * host_ts).exp();
        let psi1 = (pole_corr - Complex32::new(1.0, 0.0)) / pole;
        Self {
            pole,
            host_ts,
            pole_corr,
            psi1,
            x: Complex32::new(0.0, 0.0),
        }
    }

    pub fn pole(&self) -> Complex32 {
        self.pole
    }

    pub fn pole_corr(&self) -> Complex32 {
        self.pole_corr
    }

    pub fn state(&self) -> Complex32 {
        self.x
    }

    /// Closed-form `φ(τ) = exp(p · τ · Ts)` — the decay/rotation factor
    /// for sub-sample time `τ ∈ [0, 1]`. Used to compute an impulse's
    /// residual contribution at the end of the host sample:
    /// `contribution = φ(1 - τ) · impulse_value`.
    pub fn phi(&self, tau: f32) -> Complex32 {
        (self.pole * (tau * self.host_ts)).exp()
    }

    /// Replace the state — for tests and external drivers that bypass
    /// the normal `advance`/`evaluate` flow.
    pub fn set_state(&mut self, x: Complex32) {
        self.x = x;
    }

    pub fn reset(&mut self) {
        self.x = Complex32::new(0.0, 0.0);
    }

    /// Evaluate `y(n·Ts + τ·Ts)` given input `u` held for the current
    /// sample. `τ ∈ [0, 1]` — at `τ = 0` the state-contribution
    /// dominates; at `τ = 1` this matches the post-advance state.
    pub fn evaluate(&self, tau: f32, u: f32) -> Complex32 {
        let phi = (self.pole * (tau * self.host_ts)).exp();
        let psi = (phi - Complex32::new(1.0, 0.0)) / self.pole;
        phi * self.x + psi * Complex32::new(u, 0.0)
    }

    /// Roll state forward one host sample with input `u` held over
    /// `[n·Ts, (n+1)·Ts)`.
    pub fn advance(&mut self, u: f32) {
        self.x = self.pole_corr * self.x + self.psi1 * Complex32::new(u, 0.0);
    }

    /// Evolve state by a fraction `Δτ ∈ [0, 1]` of a host sample with
    /// `u` held constant throughout the interval. Closed-form:
    /// `x_new = φ(Δτ)·x + ψ(Δτ)·u`. Useful for output reconstruction
    /// where the input to the filter changes at sub-sample Read-tick
    /// boundaries.
    pub fn advance_by(&mut self, delta_tau: f32, u: f32) {
        let phi = (self.pole * (delta_tau * self.host_ts)).exp();
        let psi = (phi - Complex32::new(1.0, 0.0)) / self.pole;
        self.x = phi * self.x + psi * Complex32::new(u, 0.0);
    }
}

/// A bank of complex one-poles with real residues summed at output.
/// Real poles must come as conjugate pairs for real-valued output.
#[derive(Clone, Debug)]
pub struct ContinuousPoleBank {
    poles: Vec<ContinuousPole>,
    residues: Vec<Complex32>,
}

impl ContinuousPoleBank {
    pub fn new(
        poles: impl IntoIterator<Item = Complex32>,
        residues: impl IntoIterator<Item = Complex32>,
        sample_rate: f32,
    ) -> Self {
        let poles: Vec<_> = poles
            .into_iter()
            .map(|p| ContinuousPole::new(p, sample_rate))
            .collect();
        let residues: Vec<_> = residues.into_iter().collect();
        assert_eq!(poles.len(), residues.len());
        Self { poles, residues }
    }

    /// `H(s) = Σ r_k / (s - p_k)` evaluated at sub-sample `τ`.
    pub fn evaluate(&self, tau: f32, u: f32) -> f32 {
        let mut sum = Complex32::new(0.0, 0.0);
        for (p, r) in self.poles.iter().zip(self.residues.iter()) {
            sum += *r * p.evaluate(tau, u);
        }
        sum.re
    }

    pub fn advance(&mut self, u: f32) {
        for p in self.poles.iter_mut() {
            p.advance(u);
        }
    }

    /// Evolve every pole by a sub-sample fraction with `u` held. See
    /// [`ContinuousPole::advance_by`].
    pub fn advance_by(&mut self, delta_tau: f32, u: f32) {
        for p in self.poles.iter_mut() {
            p.advance_by(delta_tau, u);
        }
    }

    pub fn reset(&mut self) {
        for p in self.poles.iter_mut() {
            p.reset();
        }
    }

    pub fn pole_count(&self) -> usize {
        self.poles.len()
    }

    pub fn poles(&self) -> &[ContinuousPole] {
        &self.poles
    }

    pub fn poles_mut(&mut self) -> &mut [ContinuousPole] {
        &mut self.poles
    }

    pub fn residues(&self) -> &[Complex32] {
        &self.residues
    }

    /// Sum `Σ r_k · x_k` (complex) and return the real part. Useful
    /// when the bank is driven externally via per-pole state
    /// manipulation rather than the built-in `advance(u)`.
    pub fn real_output(&self) -> f32 {
        let mut sum = Complex32::default();
        for (pole, r) in self.poles.iter().zip(self.residues.iter()) {
            sum += *r * pole.state();
        }
        sum.re
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const TS: f32 = 1.0 / SR;

    fn assert_close(got: Complex32, want: Complex32, tol: f32, ctx: &str) {
        let dr = (got.re - want.re).abs();
        let di = (got.im - want.im).abs();
        assert!(
            dr < tol && di < tol,
            "{ctx}: got ({}, {}), want ({}, {}), |d| = ({dr}, {di})",
            got.re,
            got.im,
            want.re,
            want.im
        );
    }

    // ─── Level 1: filter in isolation vs closed-form ─────────────────────────

    #[test]
    fn impulse_at_sample_zero_matches_closed_form() {
        // u[0] = 1, u[n] = 0 for n > 0.
        // At sample 0 with u=1: y(τ·Ts) = ψ(τ) = (exp(p·τ·Ts) - 1) / p
        let p = Complex32::new(-10_000.0, 3_000.0);
        let mut f = ContinuousPole::new(p, SR);
        for tau in [0.0_f32, 0.25, 0.5, 0.75, 0.99] {
            let got = f.evaluate(tau, 1.0);
            let phi = (p * (tau * TS)).exp();
            let want = (phi - Complex32::new(1.0, 0.0)) / p;
            assert_close(got, want, 1.0e-6, &format!("sample 0 τ={tau}"));
        }
        f.advance(1.0);
        // After advance, state x[1] = ψ(1). Subsequent samples with
        // u=0: y((n+τ)·Ts) = φ(τ) · pole_corr^(n-1) · ψ(1).
        for n in 1..5 {
            for tau in [0.0_f32, 0.5, 0.99] {
                let got = f.evaluate(tau, 0.0);
                let phi = (p * (tau * TS)).exp();
                let want =
                    phi * f.pole_corr.powf((n - 1) as f32) * f.psi1;
                assert_close(got, want, 1.0e-5, &format!("sample {n} τ={tau}"));
            }
            f.advance(0.0);
        }
    }

    #[test]
    fn dc_steady_state_matches_theory() {
        // For sustained u = U, steady state satisfies 0 = p·x + U,
        // so x_ss = -U/p. Input residue is 1/(s-p), DC gain = -1/p.
        let p = Complex32::new(-1_000.0, 500.0);
        let mut f = ContinuousPole::new(p, SR);
        let u = 1.0_f32;
        // Decay time: 1/Re(p) = 1 ms; 100 ms is 100τ, well settled.
        for _ in 0..((SR * 0.1) as usize) {
            f.advance(u);
        }
        let want = -Complex32::new(u, 0.0) / p;
        assert_close(f.state(), want, 1.0e-4, "DC steady state");
    }

    // ─── Level 2: stitch consistency ─────────────────────────────────────────

    #[test]
    fn evaluate_tau_one_equals_next_sample_tau_zero() {
        // Continuous-time output is continuous across held-input
        // boundaries even when u changes. Verify:
        //   evaluate(1, u[n])  ==  advance(u[n]); evaluate(0, u[n+1])
        let p = Complex32::new(-10_000.0, 7_500.0);
        let mut f = ContinuousPole::new(p, SR);
        let inputs = [1.0_f32, 0.3, -0.7, 0.0, 0.5, -0.2];
        for pair in inputs.windows(2) {
            let u_n = pair[0];
            let u_np1 = pair[1];
            let at_end = f.evaluate(1.0, u_n);
            f.advance(u_n);
            let at_start = f.evaluate(0.0, u_np1);
            assert_close(at_end, at_start, 1.0e-6, "stitch");
        }
    }

    #[test]
    fn advance_equals_evaluate_at_tau_one() {
        let p = Complex32::new(-5_000.0, 20_000.0);
        let mut f = ContinuousPole::new(p, SR);
        let u = 0.7_f32;
        let want = f.evaluate(1.0, u);
        f.advance(u);
        assert_close(f.state(), want, 1.0e-6, "advance == evaluate(1)");
    }

    #[test]
    fn sub_sample_evaluation_is_monotonic_decay_in_magnitude() {
        // After a single impulse, the filter's magnitude decays like
        // |exp(Re(p)·t)|. Check monotonicity across sub-samples.
        let p = Complex32::new(-20_000.0, 1_000.0);
        let mut f = ContinuousPole::new(p, SR);
        f.advance(1.0);
        f.advance(0.0);
        // Now evaluate a dense grid within sample n=1 (u=0) and
        // assert the magnitude is non-increasing.
        let mut prev_mag = f32::INFINITY;
        for i in 0..20 {
            let tau = i as f32 / 19.0;
            let val = f.evaluate(tau, 0.0);
            let mag = (val.re * val.re + val.im * val.im).sqrt();
            assert!(
                mag <= prev_mag * 1.000_001,
                "magnitude should not grow: τ={tau} mag={mag} prev={prev_mag}"
            );
            prev_mag = mag;
        }
    }

    // ─── Level 3: cross-check against host-rate IIR ──────────────────────────

    #[test]
    fn host_rate_samples_match_impulse_invariant_iir() {
        // `evaluate(0)` at each sample should trace the exact IIR
        // recurrence `y[n+1] = pole_corr · y[n] + ψ1 · u[n]`. That's
        // the discrete state advance, so it's also a consistency
        // check against `advance`.
        let p = Complex32::new(-12_000.0, 8_000.0);
        let mut f = ContinuousPole::new(p, SR);
        let mut y = Complex32::new(0.0, 0.0);
        let inputs: Vec<f32> =
            (0..200).map(|i| (i as f32 * 0.03).sin()).collect();
        for &u in &inputs {
            let at_start = f.evaluate(0.0, u);
            assert_close(at_start, y, 1.0e-5, "start of sample == state");
            y = f.pole_corr * y + f.psi1 * Complex32::new(u, 0.0);
            f.advance(u);
        }
    }

    // ─── Bank tests ──────────────────────────────────────────────────────────

    #[test]
    fn conjugate_pole_pair_gives_real_output() {
        // A pole `p` with residue `r` and its conjugate `p*` with
        // residue `r*` produces a purely real response for real input.
        let p = Complex32::new(-5_000.0, 12_000.0);
        let r = Complex32::new(1.0, 2.0);
        let bank = ContinuousPoleBank::new(
            [p, p.conj()],
            [r, r.conj()],
            SR,
        );
        // Real output only makes sense via the bank's evaluate, which
        // takes Re(sum). Check that summing without Re would still be
        // real anyway — i.e. the imag part cancels.
        let mut bank2 = bank.clone();
        bank2.advance(1.0);
        bank2.advance(0.0);
        let got = bank2.evaluate(0.5, 0.0);
        // Manually sum full complex to verify imag ≈ 0.
        let mut sum = Complex32::new(0.0, 0.0);
        let pc = ContinuousPole::new(p, SR);
        let pc_conj = ContinuousPole::new(p.conj(), SR);
        // Re-run scalar pair to mirror bank2's state.
        let (mut x0, mut x1) = (Complex32::new(0.0, 0.0), Complex32::new(0.0, 0.0));
        for &u in &[1.0_f32, 0.0] {
            x0 = pc.pole_corr * x0 + pc.psi1 * Complex32::new(u, 0.0);
            x1 = pc_conj.pole_corr * x1 + pc_conj.psi1 * Complex32::new(u, 0.0);
        }
        let phi0 = (pc.pole() * (0.5 * TS)).exp();
        let phi1 = (pc_conj.pole() * (0.5 * TS)).exp();
        sum += r * (phi0 * x0);
        sum += r.conj() * (phi1 * x1);
        assert!(
            sum.im.abs() < 1.0e-4,
            "conjugate pair should cancel imag, got {}",
            sum.im
        );
        // Bank's `evaluate` drops the imag part; check real agrees.
        assert!((got - sum.re).abs() < 1.0e-4, "bank.re {got} vs hand-summed {}", sum.re);
    }
}
