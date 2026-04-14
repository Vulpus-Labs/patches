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
    f_coeff: f32,
    f_target: f32,
    df: f32,
    q_damp: f32,
    q_damp_target: f32,
    dq: f32,
    state: SvfState,
}

impl SvfKernel {
    /// Create a new kernel with static (non-ramping) coefficients.
    pub fn new_static(f: f32, d: f32) -> Self {
        let f = stability_clamp(f, d);
        Self {
            f_coeff: f,
            f_target: f,
            df: 0.0,
            q_damp: d,
            q_damp_target: d,
            dq: 0.0,
            state: SvfState::default(),
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
        self.f_coeff = f;
        self.f_target = f;
        self.df = 0.0;
        self.q_damp = d;
        self.q_damp_target = d;
        self.dq = 0.0;
    }

    /// Snap active coefficients to the previous targets, store new targets,
    /// and compute per-sample deltas for the next update window.
    ///
    /// `interval_recip` is `1.0 / periodic_update_interval`, precomputed by
    /// the caller to avoid a division in the audio path.
    pub fn begin_ramp(&mut self, ft: f32, dt: f32, interval_recip: f32) {
        let ft = stability_clamp(ft, dt);
        self.f_coeff = stability_clamp(self.f_target, self.q_damp_target);
        self.q_damp = self.q_damp_target;
        self.f_target = ft;
        self.q_damp_target = dt;
        self.df = (ft - self.f_coeff) * interval_recip;
        self.dq = (dt - self.q_damp) * interval_recip;
    }

    /// Reset integrator state to zero without touching coefficients.
    pub fn reset_state(&mut self) {
        self.state.reset();
    }

    /// Run one Chamberlin SVF sample, advance interpolating coefficients,
    /// and return `(lp, hp, bp)`.
    #[inline]
    pub fn tick(&mut self, x: f32) -> (f32, f32, f32) {
        let lp = self.state.lp + self.f_coeff * self.state.bp;
        let hp = x - lp - self.q_damp * self.state.bp;
        let bp = self.state.bp + self.f_coeff * hp;
        self.state.lp = sanitize(lp);
        self.state.bp = sanitize(bp);
        self.f_coeff += self.df;
        self.q_damp += self.dq;
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
    // ── Hot: active coefficients ─────────────────────────────────────────
    f_coeff: [f32; 16],
    df: [f32; 16],
    q_damp: [f32; 16],
    dq: [f32; 16],
    // ── Hot: integrator state ─────────────────────────────────────────────
    lp_state: [f32; 16],
    bp_state: [f32; 16],
    // ── Cold: targets (read only at update boundaries) ────────────────────
    f_target: [f32; 16],
    q_damp_target: [f32; 16],
}

impl PolySvfKernel {
    /// Create a new kernel with all 16 voices set to the same static coefficients.
    pub fn new_static(f: f32, d: f32) -> Self {
        let f = stability_clamp(f, d);
        Self {
            f_coeff: [f; 16],
            df: [0.0; 16],
            q_damp: [d; 16],
            dq: [0.0; 16],
            lp_state: [0.0; 16],
            bp_state: [0.0; 16],
            f_target: [f; 16],
            q_damp_target: [d; 16],
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
        self.f_coeff = [f; 16];
        self.df = [0.0; 16];
        self.q_damp = [d; 16];
        self.dq = [0.0; 16];
        self.f_target = [f; 16];
        self.q_damp_target = [d; 16];
    }

    /// Snap voice `i` to its stored targets, store new targets, compute deltas.
    ///
    /// `interval_recip` is `1.0 / periodic_update_interval`, precomputed by
    /// the caller.
    pub fn begin_ramp_voice(&mut self, i: usize, ft: f32, dt: f32, interval_recip: f32) {
        let ft = stability_clamp(ft, dt);
        self.f_coeff[i] = stability_clamp(self.f_target[i], self.q_damp_target[i]);
        self.q_damp[i] = self.q_damp_target[i];
        self.f_target[i] = ft;
        self.q_damp_target[i] = dt;
        self.df[i] = (ft - self.f_coeff[i]) * interval_recip;
        self.dq[i] = (dt - self.q_damp[i]) * interval_recip;
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
        // Step 1: lp = lp_state + f_coeff * bp_state  — independent across voices
        let lp: [f32; 16] =
            std::array::from_fn(|i| self.lp_state[i] + self.f_coeff[i] * self.bp_state[i]);
        // Step 2: hp = x - lp - q_damp * bp_state  — depends on lp[], not lp[i±1]
        let hp: [f32; 16] =
            std::array::from_fn(|i| x[i] - lp[i] - self.q_damp[i] * self.bp_state[i]);
        // Step 3: bp = bp_state + f_coeff * hp
        let bp: [f32; 16] =
            std::array::from_fn(|i| self.bp_state[i] + self.f_coeff[i] * hp[i]);
        // State update (sanitize to prevent NaN/Inf propagation)
        self.lp_state = std::array::from_fn(|i| sanitize(lp[i]));
        self.bp_state = std::array::from_fn(|i| sanitize(bp[i]));
        // Step 4 (CV path only): advance active coefficients
        if ramp {
            for i in 0..16 {
                self.f_coeff[i] += self.df[i];
                self.q_damp[i] += self.dq[i];
            }
        }
        (lp, hp, bp)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_within;
    use std::f32::consts::PI;

    const SAMPLE_RATE: f32 = 48_000.0;

    fn make_kernel(cutoff_hz: f32, q_norm: f32) -> SvfKernel {
        let f = svf_f(cutoff_hz, SAMPLE_RATE);
        let d = q_to_damp(q_norm);
        SvfKernel::new_static(f, d)
    }

    // ── T1 — Impulse response ────────────────────────────────────────────────

    /// Process a unit impulse through a known SVF setting and assert that the
    /// lowpass output matches a reference computed from the closed-form
    /// recurrence within tolerance 1e-9.
    #[test]
    fn t1_impulse_response_lowpass() {
        let fc = 1_000.0_f32;
        let q_norm = 0.5_f32;
        let f = svf_f(fc, SAMPLE_RATE);
        let d = q_to_damp(q_norm);

        let mut kernel = SvfKernel::new_static(f, d);

        // Reference: compute the same recurrence manually
        let mut ref_lp = 0.0_f32;
        let mut ref_bp = 0.0_f32;

        let n_samples = 64;
        for i in 0..n_samples {
            let x = if i == 0 { 1.0_f32 } else { 0.0_f32 };

            // Manual recurrence
            let ref_lp_new = ref_lp + f * ref_bp;
            let ref_hp = x - ref_lp_new - d * ref_bp;
            let ref_bp_new = ref_bp + f * ref_hp;
            ref_lp = ref_lp_new;
            ref_bp = ref_bp_new;

            let (lp, _hp, _bp) = kernel.tick(x);
            assert!(
                (lp - ref_lp).abs() < 1e-9,
                "sample {i}: lp={lp}, ref={ref_lp}, diff={}",
                (lp - ref_lp).abs()
            );
        }
    }

    // ── T2 — Frequency response ──────────────────────────────────────────────

    /// Drive a sinusoid at the cutoff frequency and measure steady-state
    /// amplitude; compare against the theoretical transfer function magnitude.
    ///
    /// The Chamberlin SVF bandpass peak is at the design frequency and has
    /// magnitude ≈ 1 / q_damp at resonance.  For lowpass and highpass the
    /// amplitude passes through a predictable level.  We allow ±1 dB error to
    /// account for the bilinear/numerical approximation.
    fn db(ratio: f32) -> f32 {
        20.0 * ratio.log10()
    }

    fn measure_steady_state_amplitude(
        kernel: &mut SvfKernel,
        freq_hz: f32,
        mode_fn: fn((f32, f32, f32)) -> f32,
    ) -> f32 {
        let omega = 2.0 * PI * freq_hz / SAMPLE_RATE;
        // Warm-up: 4096 samples to reach steady state
        for i in 0..4096_usize {
            let x = (omega * i as f32).sin();
            let out = kernel.tick(x);
            let _ = mode_fn(out);
        }
        // Measurement: accumulate peak over 1024 samples
        let mut peak = 0.0_f32;
        for i in 4096..5120_usize {
            let x = (omega * i as f32).sin();
            let out = kernel.tick(x);
            let y = mode_fn(out);
            if y.abs() > peak {
                peak = y.abs();
            }
        }
        peak
    }

    /// T2-LP: Lowpass at 100 Hz drive (well below 1 kHz cutoff) should pass ≈ 1.0.
    #[test]
    fn t2_frequency_response_lowpass_passband() {
        let fc = 1_000.0_f32;
        let q_norm = 0.0_f32; // flat/Butterworth
        let mut kernel = make_kernel(fc, q_norm);

        let drive_hz = 100.0;
        let amp = measure_steady_state_amplitude(&mut kernel, drive_hz, |(lp, _, _)| lp);

        // Passband: amplitude should be within ±1 dB of 1.0
        let db_err = db(amp).abs();
        assert!(
            db_err < 1.0,
            "LP passband at {drive_hz} Hz: amplitude={amp:.4}, dB_from_unity={db_err:.3}"
        );
    }

    /// T2-HP: Highpass at 10 kHz drive (well above 1 kHz cutoff) should be in the
    /// passband (within ±3 dB of unity).
    ///
    /// The Chamberlin SVF topology has a frequency-warping approximation that causes
    /// a slight amplitude overshoot at high frequencies, so we allow ±3 dB here.
    #[test]
    fn t2_frequency_response_highpass_passband() {
        let fc = 1_000.0_f32;
        let q_norm = 0.0_f32;
        let mut kernel = make_kernel(fc, q_norm);

        let drive_hz = 10_000.0;
        let amp = measure_steady_state_amplitude(&mut kernel, drive_hz, |(_, hp, _)| hp);

        // Must be in passband: amplitude between -3 dB and +3 dB of unity
        assert!(
            amp > 0.7 && amp < 1.5,
            "HP passband at {drive_hz} Hz: amplitude={amp:.4}, expected in [0.7, 1.5]"
        );
    }

    /// T2-BP: Bandpass at cutoff frequency should peak near 1/q_damp.
    #[test]
    fn t2_frequency_response_bandpass_peak() {
        let fc = 1_000.0_f32;
        let q_norm = 0.5_f32;
        let f = svf_f(fc, SAMPLE_RATE);
        let d = q_to_damp(q_norm);
        let mut kernel = SvfKernel::new_static(f, d);

        // Drive at exact cutoff
        let amp = measure_steady_state_amplitude(&mut kernel, fc, |(_, _, bp)| bp);
        let theoretical = 1.0 / d; // peak gain at resonance
        let ratio = amp / theoretical;
        let db_err = db(ratio).abs();
        assert!(
            db_err < 1.0,
            "BP peak at fc={fc} Hz: amplitude={amp:.4}, theoretical={theoretical:.4}, dB_err={db_err:.3}"
        );
    }

    // ── T3 — DC and Nyquist ──────────────────────────────────────────────────

    /// T3-DC-LP: Lowpass passes DC ≈ 1.0.
    #[test]
    fn t3_dc_lowpass_passes() {
        let fc = 1_000.0_f32;
        let q_norm = 0.0_f32;
        let mut kernel = make_kernel(fc, q_norm);

        // Warm up with DC input
        let mut lp_out = 0.0_f32;
        for _ in 0..48_000_usize {
            (lp_out, _, _) = kernel.tick(1.0);
        }
        assert!(
            (lp_out - 1.0).abs() < 1e-3,
            "LP DC output should be ≈1.0, got {lp_out}"
        );
    }

    /// T3-DC-HP: Highpass rejects DC ≈ 0.0.
    #[test]
    fn t3_dc_highpass_rejects() {
        let fc = 1_000.0_f32;
        let q_norm = 0.0_f32;
        let mut kernel = make_kernel(fc, q_norm);

        let mut hp_out = 0.0_f32;
        for _ in 0..48_000_usize {
            (_, hp_out, _) = kernel.tick(1.0);
        }
        assert!(
            hp_out.abs() < 1e-3,
            "HP DC output should be ≈0.0, got {hp_out}"
        );
    }

    /// T3-Nyquist: Highpass passes Nyquist (alternating ±1) with significant amplitude.
    ///
    /// The Chamberlin SVF has a slight overshoot near Nyquist due to the
    /// sinc-based frequency approximation. We assert the output is well above 0.5
    /// (clearly in the passband), rather than requiring exactly 1.0.
    #[test]
    fn t3_nyquist_highpass_passes() {
        let fc = 1_000.0_f32;
        let q_norm = 0.0_f32;
        let mut kernel = make_kernel(fc, q_norm);

        // Warm up with alternating signal
        let mut peak = 0.0_f32;
        for i in 0..4096_usize {
            let x = if i % 2 == 0 { 1.0_f32 } else { -1.0_f32 };
            let (_, hp, _) = kernel.tick(x);
            if i > 2048
                && hp.abs() > peak {
                    peak = hp.abs();
                }
        }
        // Chamberlin SVF HP at Nyquist should be in passband (> 0.5) but may overshoot
        assert!(
            peak > 0.5,
            "HP Nyquist amplitude should be >0.5 (in passband), got {peak}"
        );
    }

    // ── T4 — Stability under high resonance ──────────────────────────────────

    /// Run SVF at high resonance (Q≈10, q_norm≈0.83) for 10,000 samples with a
    /// unit-impulse input; assert output is bounded (|y| < 100).
    #[test]
    fn t4_stability_high_resonance() {
        // q_norm=0.83 gives damping ≈ 2*0.005^0.83 ≈ 0.1, i.e. Q ≈ 10
        let q_norm = 0.83_f32;
        let fc = 1_000.0_f32;
        let f = svf_f(fc, SAMPLE_RATE);
        let d = q_to_damp(q_norm);
        let mut kernel = SvfKernel::new_static(f, d);

        for i in 0..10_000_usize {
            let x = if i == 0 { 1.0_f32 } else { 0.0_f32 };
            let (lp, hp, bp) = kernel.tick(x);
            assert!(
                lp.abs() < 100.0 && hp.abs() < 100.0 && bp.abs() < 100.0,
                "sample {i}: SVF output unbounded: lp={lp}, hp={hp}, bp={bp}"
            );
        }
    }

    // ── T5 — Stability under ADSR-driven FM sweep at high Q ──────────────────

    /// Simulate an ADSR envelope driving the FM input while Q is near max.
    /// Before the stability clamp this produced NaN within a few hundred
    /// samples; after the fix all outputs must remain finite and bounded.
    #[test]
    fn t5_stability_adsr_fm_sweep_high_q() {
        let q_norm = 0.95_f32;
        let base_cutoff_voct = 6.0_f32; // ~1047 Hz
        let c0_freq = 16.351_599_f32;

        let d = q_to_damp(q_norm);
        let base_fc = (c0_freq * base_cutoff_voct.exp2()).clamp(1.0, SAMPLE_RATE * 0.499);
        let mut kernel = SvfKernel::new_static(svf_f(base_fc, SAMPLE_RATE), d);

        let interval = 32_usize;
        let recip = 1.0 / interval as f32;

        // Simulate a fast ADSR attack (0→1 in 32 ms ≈ 1536 samples at 48 kHz)
        // followed by a sustain, sweeping cutoff up by 4 octaves.
        let total = 10_000_usize;
        let attack_samples = 1536_usize;
        for n in 0..total {
            // Update coefficients every `interval` samples.
            if n % interval == 0 {
                let env = if n < attack_samples {
                    n as f32 / attack_samples as f32
                } else {
                    1.0
                };
                let fc = (c0_freq * (base_cutoff_voct + env * 4.0).exp2())
                    .clamp(1.0, SAMPLE_RATE * 0.499);
                let ft = svf_f(fc, SAMPLE_RATE);
                kernel.begin_ramp(ft, d, recip);
            }
            let x = if n < 64 { 0.5_f32 } else { 0.0 };
            let (lp, hp, bp) = kernel.tick(x);
            assert!(
                lp.is_finite() && hp.is_finite() && bp.is_finite(),
                "sample {n}: NaN/Inf detected: lp={lp}, hp={hp}, bp={bp}"
            );
            assert!(
                lp.abs() < 1e6 && hp.abs() < 1e6 && bp.abs() < 1e6,
                "sample {n}: runaway output: lp={lp}, hp={hp}, bp={bp}"
            );
        }
    }

    /// Same scenario as T5 but for the 16-voice PolySvfKernel.
    #[test]
    fn t5_poly_stability_adsr_fm_sweep_high_q() {
        let q_norm = 0.95_f32;
        let base_cutoff_voct = 6.0_f32;
        let c0_freq = 16.351_599_f32;

        let d = q_to_damp(q_norm);
        let base_fc = (c0_freq * base_cutoff_voct.exp2()).clamp(1.0, SAMPLE_RATE * 0.499);
        let mut kernel = PolySvfKernel::new_static(svf_f(base_fc, SAMPLE_RATE), d);

        let interval = 32_usize;
        let recip = 1.0 / interval as f32;
        let total = 10_000_usize;
        let attack_samples = 1536_usize;

        for n in 0..total {
            if n % interval == 0 {
                let env = if n < attack_samples {
                    n as f32 / attack_samples as f32
                } else {
                    1.0
                };
                let fc = (c0_freq * (base_cutoff_voct + env * 4.0).exp2())
                    .clamp(1.0, SAMPLE_RATE * 0.499);
                let ft = svf_f(fc, SAMPLE_RATE);
                for i in 0..16 {
                    kernel.begin_ramp_voice(i, ft, d, recip);
                }
            }
            let x: [f32; 16] = if n < 64 { [0.5; 16] } else { [0.0; 16] };
            let (lp, hp, bp) = kernel.tick_all(&x, true);
            for i in 0..16 {
                assert!(
                    lp[i].is_finite() && hp[i].is_finite() && bp[i].is_finite(),
                    "sample {n} voice {i}: NaN/Inf: lp={}, hp={}, bp={}",
                    lp[i], hp[i], bp[i]
                );
            }
        }
    }

    // ── T6 — SNR and precision ───────────────────────────────────────────────

    /// T6 — SNR and precision
    ///
    /// Run an SvfKernel (1000 Hz cutoff, q_norm = 0.5) on 10,000 samples of a
    /// 200 Hz sinusoid at 48 kHz in both f32 and an inline f64 Chamberlin SVF
    /// reference.  Compute RMS error on the lowpass output and assert SNR ≥ 60 dB.
    #[test]
    fn t6_snr_svf_lp_vs_f64_reference() {
        use std::f64::consts::PI as PI64;

        const SR: f32 = 48_000.0;
        const SR64: f64 = 48_000.0;
        const FC: f32 = 1_000.0;
        const Q_NORM: f32 = 0.5;
        const DRIVE_HZ: f64 = 200.0;
        const N: usize = 10_000;

        let f32_coeff = svf_f(FC, SR);
        let d32_coeff = q_to_damp(Q_NORM);
        let mut kernel = SvfKernel::new_static(f32_coeff, d32_coeff);

        // f64 coefficients — mirror the same formulas with f64 precision.
        let f64_coeff: f64 = 2.0 * (PI64 * FC as f64 / SR64).sin();
        let d64_coeff: f64 = 2.0 * (0.005_f64).powf(Q_NORM as f64);

        let mut ref_lp = 0.0_f64;
        let mut ref_bp = 0.0_f64;

        let mut sum_sq_signal = 0.0_f64;
        let mut sum_sq_error = 0.0_f64;

        for k in 0..N {
            let x64 = (2.0 * PI64 * DRIVE_HZ / SR64 * k as f64).sin();
            let x32 = x64 as f32;

            // f64 Chamberlin SVF recurrence.
            let lp_new = ref_lp + f64_coeff * ref_bp;
            let hp_new = x64 - lp_new - d64_coeff * ref_bp;
            let bp_new = ref_bp + f64_coeff * hp_new;
            ref_lp = lp_new;
            ref_bp = bp_new;

            // f32 kernel.
            let (lp32, _hp32, _bp32) = kernel.tick(x32);

            sum_sq_signal += ref_lp * ref_lp;
            let err = lp32 as f64 - ref_lp;
            sum_sq_error += err * err;
        }

        let rms_signal = (sum_sq_signal / N as f64).sqrt();
        let rms_error = (sum_sq_error / N as f64).sqrt();
        let snr_db = 20.0 * (rms_signal / rms_error).log10();

        // Measured 141.7 dB on aarch64 macOS debug (2026-04-02). Tightened from 60 dB.
        assert!(
            snr_db >= 120.0,
            "SNR too low: {snr_db:.1} dB (expected ≥ 120 dB); rms_signal={rms_signal:.6}, rms_error={rms_error:.6}"
        );
    }

    // ── T7 — Determinism ─────────────────────────────────────────────────────

    /// Same input twice with state reset → bit-identical output.
    #[test]
    fn t7_determinism() {
        use crate::test_support::assert_deterministic;

        let fc = 800.0_f32;
        let q_norm = 0.4_f32;
        let f = svf_f(fc, SAMPLE_RATE);
        let d = q_to_damp(q_norm);

        let input: Vec<f32> = (0..256)
            .map(|i| (2.0 * PI * 440.0 / SAMPLE_RATE * i as f32).sin())
            .collect();

        assert_deterministic!(
            SvfKernel::new_static(f, d),
            &input,
            |k: &mut SvfKernel, x: f32| { let (lp, _hp, _bp) = k.tick(x); lp }
        );
    }

    // ── PolySvfKernel: basic parity with SvfKernel ───────────────────────────

    /// All 16 voices of PolySvfKernel should produce identical output to
    /// SvfKernel when driven with the same coefficients and input.
    #[test]
    fn poly_kernel_matches_mono_kernel() {
        let fc = 500.0_f32;
        let q_norm = 0.3_f32;
        let f = svf_f(fc, SAMPLE_RATE);
        let d = q_to_damp(q_norm);

        let mut mono = SvfKernel::new_static(f, d);
        let mut poly = PolySvfKernel::new_static(f, d);

        for i in 0..512_usize {
            let x = (2.0 * PI * 300.0 / SAMPLE_RATE * i as f32).sin();
            let xs = [x; 16];

            let (mlp, mhp, mbp) = mono.tick(x);
            let (lp_arr, hp_arr, bp_arr) = poly.tick_all(&xs, false);

            for v in 0..16 {
                assert!(
                    (lp_arr[v] - mlp).abs() < 1e-9,
                    "voice {v} sample {i}: lp mismatch: {}/{mlp}",
                    lp_arr[v]
                );
                assert!(
                    (hp_arr[v] - mhp).abs() < 1e-9,
                    "voice {v} sample {i}: hp mismatch: {}/{mhp}",
                    hp_arr[v]
                );
                assert!(
                    (bp_arr[v] - mbp).abs() < 1e-9,
                    "voice {v} sample {i}: bp mismatch: {}/{mbp}",
                    bp_arr[v]
                );
            }
        }
    }

    // ── PolySvfKernel: additional coverage ─────────────────────────────────────

    /// Two voices driven with different frequencies produce divergent output.
    #[test]
    fn poly_svf_voices_are_independent() {
        let f0 = svf_f(500.0, SAMPLE_RATE);
        let f1 = svf_f(5000.0, SAMPLE_RATE);
        let d = q_to_damp(0.3);

        let mut poly = PolySvfKernel::new_static(f0, d);
        // Set voice 1 to a different frequency
        poly.f_coeff[1] = f1;
        poly.f_target[1] = f1;

        let mut input = [0.0f32; 16];
        // Drive both voices with the same signal
        for i in 0..512 {
            let x = (2.0 * PI * 300.0 / SAMPLE_RATE * i as f32).sin();
            input.fill(x);
            poly.tick_all(&input, false);
        }

        // After processing, voice 0 and voice 1 should have different state
        assert!(
            (poly.lp_state[0] - poly.lp_state[1]).abs() > 1e-6,
            "voices should diverge: lp[0]={}, lp[1]={}",
            poly.lp_state[0], poly.lp_state[1]
        );
    }

    /// Two identical poly kernels produce bit-identical output.
    #[test]
    fn poly_svf_determinism() {
        let f = svf_f(800.0, SAMPLE_RATE);
        let d = q_to_damp(0.4);

        let mut poly_a = PolySvfKernel::new_static(f, d);
        let mut poly_b = PolySvfKernel::new_static(f, d);

        for i in 0..256 {
            let x = (2.0 * PI * 440.0 / SAMPLE_RATE * i as f32).sin();
            let xs = [x; 16];
            let (lp_a, hp_a, bp_a) = poly_a.tick_all(&xs, false);
            let (lp_b, hp_b, bp_b) = poly_b.tick_all(&xs, false);
            assert_eq!(lp_a, lp_b, "lp mismatch at sample {i}");
            assert_eq!(hp_a, hp_b, "hp mismatch at sample {i}");
            assert_eq!(bp_a, bp_b, "bp mismatch at sample {i}");
        }
    }

    /// Resetting state zeroes integrators without affecting coefficients.
    #[test]
    fn poly_svf_reset() {
        let f = svf_f(1000.0, SAMPLE_RATE);
        let d = q_to_damp(0.5);
        let mut poly = PolySvfKernel::new_static(f, d);

        // Feed signal to build up state
        for i in 0..100 {
            let x = (2.0 * PI * 300.0 / SAMPLE_RATE * i as f32).sin();
            poly.tick_all(&[x; 16], false);
        }
        assert!(poly.lp_state[0] != 0.0, "state should be non-zero after processing");

        poly.reset_state();

        for v in 0..16 {
            assert_eq!(poly.lp_state[v], 0.0, "voice {v} lp not reset");
            assert_eq!(poly.bp_state[v], 0.0, "voice {v} bp not reset");
        }
        // Coefficients should be untouched
        assert_eq!(poly.f_coeff[0], f);
        assert_eq!(poly.q_damp[0], d);
    }

    // ── SvfCoeffs / SvfState API ─────────────────────────────────────────────

    #[test]
    fn svf_coeffs_round_trip() {
        let c = SvfCoeffs::new(440.0, SAMPLE_RATE, 0.5);
        let mut k = SvfKernel::from_coeffs(c);
        // Just check it runs without panicking
        let _ = k.tick(1.0);
    }

    // ── FFT-based frequency response tests ─────────────────────────────────

    #[test]
    fn lowpass_frequency_response_full() {
        use crate::test_support::{assert_passband_flat, assert_stopband_below, magnitude_response_db};

        let mut kernel = make_kernel(1_000.0, 0.0);
        let fft_size = 1024;

        // Collect lowpass impulse response
        let mut ir = Vec::with_capacity(fft_size);
        for i in 0..fft_size {
            let x = if i == 0 { 1.0_f32 } else { 0.0 };
            let (lp, _, _) = kernel.tick(x);
            ir.push(lp);
        }

        let response_db = magnitude_response_db(&ir, fft_size);

        // bin_freq = bin_index * sample_rate / fft_size
        // bin_freq = bin * 48000 / 1024 ≈ bin * 46.875
        // 500 Hz → bin 10.67 → use bins 1..=10
        // 4000 Hz → bin 85.3 → use bins 86..=512
        let passband_end = (500.0 * fft_size as f32 / SAMPLE_RATE).floor() as usize; // 10
        let stopband_start = (4_000.0 * fft_size as f32 / SAMPLE_RATE).ceil() as usize; // 86
        let nyquist_bin = fft_size / 2;

        assert_passband_flat!(response_db, 1..=passband_end, 2.0);
        assert_stopband_below!(response_db, stopband_start..=nyquist_bin, -12.0);
    }

    #[test]
    fn highpass_frequency_response_full() {
        use crate::test_support::{assert_passband_flat, assert_stopband_below, magnitude_response_db};

        let mut kernel = make_kernel(1_000.0, 0.0);
        let fft_size = 1024;

        // Collect highpass impulse response
        let mut ir = Vec::with_capacity(fft_size);
        for i in 0..fft_size {
            let x = if i == 0 { 1.0_f32 } else { 0.0 };
            let (_, hp, _) = kernel.tick(x);
            ir.push(hp);
        }

        let response_db = magnitude_response_db(&ir, fft_size);

        // 200 Hz → bin 4.27 → stopband bins 1..=4
        // 4000 Hz → bin 85.3, 20000 Hz → bin 426.7
        // Use bins 86..=426 for passband
        let stopband_end = (200.0 * fft_size as f32 / SAMPLE_RATE).floor() as usize; // 4
        let passband_start = (4_000.0 * fft_size as f32 / SAMPLE_RATE).ceil() as usize; // 86
        let passband_end = (20_000.0 * fft_size as f32 / SAMPLE_RATE).floor() as usize; // 426

        assert_stopband_below!(response_db, 1..=stopband_end, -12.0);
        assert_passband_flat!(response_db, passband_start..=passband_end, 3.0);
    }

    #[test]
    fn bandpass_frequency_response_full() {
        use crate::test_support::{assert_peak_at_bin, magnitude_response_db};

        let mut kernel = make_kernel(1_000.0, 0.5);
        let fft_size = 1024;

        // Collect bandpass impulse response
        let mut ir = Vec::with_capacity(fft_size);
        for i in 0..fft_size {
            let x = if i == 0 { 1.0_f32 } else { 0.0 };
            let (_, _, bp) = kernel.tick(x);
            ir.push(bp);
        }

        let response_db = magnitude_response_db(&ir, fft_size);

        // 1000 Hz → bin 1000 * 1024 / 48000 ≈ 21.33
        let expected_bin = (1_000.0 * fft_size as f32 / SAMPLE_RATE).round() as usize; // 21
        assert_peak_at_bin!(response_db, expected_bin, 2);

        let peak_db = response_db[expected_bin];

        // Bins below 100 Hz: bin <= 2
        let low_end = (100.0 * fft_size as f32 / SAMPLE_RATE).floor() as usize; // 2
        for (bin, &v) in response_db.iter().enumerate().take(low_end + 1).skip(1) {
            assert!(
                v <= peak_db - 12.0,
                "bin {bin} at {v:.1} dB should be at least 12 dB below peak {peak_db:.1} dB"
            );
        }

        // Bins above 10 kHz: bin >= 214
        let high_start = (10_000.0 * fft_size as f32 / SAMPLE_RATE).ceil() as usize; // 214
        let nyquist_bin = fft_size / 2;
        for (bin, &v) in response_db.iter().enumerate().take(nyquist_bin + 1).skip(high_start) {
            assert!(
                v <= peak_db - 12.0,
                "bin {bin} at {v:.1} dB should be at least 12 dB below peak {peak_db:.1} dB"
            );
        }
    }

    #[test]
    fn svf_state_reset_zeroes_outputs() {
        let f = svf_f(1000.0, SAMPLE_RATE);
        let d = q_to_damp(0.5);
        let mut kernel = SvfKernel::new_static(f, d);

        // Feed signal to accumulate state
        for _ in 0..100 {
            kernel.tick(0.5);
        }
        kernel.reset_state();

        // After reset, state is zero → output at next tick driven only by input
        // lp = 0 + f*0 = 0; hp = x - 0 - d*0 = x; bp = 0 + f*x
        let x = 1.0_f32;
        let (lp, hp, bp) = kernel.tick(x);
        let expected_lp = 0.0_f32;
        let expected_hp = x; // = x - 0 - d*0
        let expected_bp = f * x;
        assert_within!(expected_lp, lp, 1e-9);
        assert_within!(expected_hp, hp, 1e-9);
        assert_within!(expected_bp, bp, 1e-9);
    }
}
