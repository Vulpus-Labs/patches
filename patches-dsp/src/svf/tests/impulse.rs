use super::*;

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
