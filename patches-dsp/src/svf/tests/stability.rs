use super::*;

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

// ── State reset: integrators zeroed without touching coefficients ────────

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
    assert_eq!(poly.coefs.active[0][0], f);
    assert_eq!(poly.coefs.active[1][0], d);
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
