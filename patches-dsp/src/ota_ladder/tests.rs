use super::*;

const SR: f32 = 48_000.0;

fn fresh(cutoff: f32, k: f32, poles: OtaPoles) -> OtaLadderKernel {
    OtaLadderKernel::new_static(OtaLadderCoeffs::new(cutoff, SR, k, 1.0), poles)
}

#[test]
fn impulse_response_is_bounded() {
    for poles in [OtaPoles::Two, OtaPoles::Four] {
        let mut k = fresh(1_000.0, 0.0, poles);
        let mut peak = 0.0f32;
        for n in 0..4096 {
            let x = if n == 0 { 1.0 } else { 0.0 };
            peak = peak.max(k.tick(x).abs());
        }
        assert!(peak.is_finite());
        assert!(peak < 1.5, "impulse peak {peak} at {poles:?}");
    }
}

#[test]
fn stable_under_max_resonance_and_drive() {
    for poles in [OtaPoles::Two, OtaPoles::Four] {
        for cutoff in [40.0, 120.0, 500.0, 2_000.0, 8_000.0, 18_000.0] {
            let mut k = OtaLadderKernel::new_static(
                OtaLadderCoeffs::new(cutoff, SR, poles.k_max(), 4.0),
                poles,
            );
            let mut peak = 0.0f32;
            for n in 0..SR as usize {
                let x = if (n / 64) % 2 == 0 { 1.0 } else { -1.0 };
                let y = k.tick(x);
                assert!(y.is_finite(), "non-finite @ {cutoff} Hz, n={n}, poles={poles:?}");
                peak = peak.max(y.abs());
            }
            assert!(peak < 4.0, "explosion at {cutoff}: peak={peak} poles={poles:?}");
        }
    }
}

#[test]
fn self_oscillates_at_k_max_4pole() {
    let mut k = fresh(1_000.0, OtaPoles::Four.k_max(), OtaPoles::Four);
    for _ in 0..16 {
        k.tick(0.5);
    }
    for _ in 0..200 {
        k.tick(0.0);
    }
    let mut peak = 0.0f32;
    for _ in 0..4_800 {
        peak = peak.max(k.tick(0.0).abs());
    }
    assert!(peak > 0.05, "4-pole failed to self-oscillate: peak={peak}");
}

#[test]
fn self_oscillates_at_k_max_2pole() {
    let mut k = fresh(1_000.0, OtaPoles::Two.k_max(), OtaPoles::Two);
    for _ in 0..16 {
        k.tick(0.5);
    }
    for _ in 0..200 {
        k.tick(0.0);
    }
    let mut peak = 0.0f32;
    for _ in 0..4_800 {
        peak = peak.max(k.tick(0.0).abs());
    }
    assert!(peak > 0.05, "2-pole failed to self-oscillate: peak={peak}");
}

#[test]
fn mode_switch_clears_feedback_tap() {
    let mut k = fresh(1_000.0, OtaPoles::Four.k_max(), OtaPoles::Four);
    for _ in 0..128 {
        k.tick(0.5);
    }
    k.set_poles(OtaPoles::Two);
    // No NaNs or runaway in the first samples after a mode change.
    for _ in 0..64 {
        let y = k.tick(0.0);
        assert!(y.is_finite());
        assert!(y.abs() < 4.0);
    }
}

#[test]
fn poly_matches_mono_with_same_input() {
    let coeffs = OtaLadderCoeffs::new(1_500.0, SR, 1.5, 1.0);
    let mut mono = OtaLadderKernel::new_static(coeffs, OtaPoles::Four);
    let mut poly = PolyOtaLadderKernel::new_static(coeffs, OtaPoles::Four);
    for n in 0..512 {
        let x = ((n as f32) * 0.01).sin();
        let voices = [x; 16];
        let ym = mono.tick(x);
        let yp = poly.tick_all(&voices, false);
        for (i, &v) in yp.iter().enumerate() {
            assert!((v - ym).abs() < 1.0e-4, "voice {i} drift: {v} vs {ym}");
        }
    }
}
