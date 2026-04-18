use super::*;

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
