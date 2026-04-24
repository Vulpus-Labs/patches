use super::*;

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
    poly.coefs.active[0][1] = f1;
    poly.targets.target[0][1] = f1;

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

// ── SvfCoeffs / SvfState API ─────────────────────────────────────────────

#[test]
fn svf_coeffs_round_trip() {
    let c = SvfCoeffs::new(440.0, SAMPLE_RATE, 0.5);
    let mut k = SvfKernel::from_coeffs(c);
    // Just check it runs without panicking
    let _ = k.tick(1.0);
}
