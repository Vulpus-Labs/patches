use super::*;

const SR: f32 = 48_000.0;

fn fresh(cutoff: f32, res: f32, variant: LadderVariant) -> LadderKernel {
    LadderKernel::new_static(LadderCoeffs::new(cutoff, SR, res, 1.0, variant))
}

#[test]
fn impulse_response_is_bounded() {
    let mut k = fresh(1_000.0, 0.0, LadderVariant::Juno60);
    let mut peak = 0.0f32;
    for n in 0..4096 {
        let x = if n == 0 { 1.0 } else { 0.0 };
        peak = peak.max(k.tick(x).abs());
    }
    assert!(peak.is_finite());
    // Input drive=1, no resonance: impulse response stays below the input amplitude.
    assert!(peak < 1.5, "impulse peak out of bounds: {peak}");
}

#[test]
fn stable_under_max_resonance_and_drive() {
    // Max res + max drive + full-scale input must not blow up across a cutoff sweep.
    for cutoff in [40.0, 120.0, 500.0, 2_000.0, 8_000.0, 18_000.0] {
        let mut k = LadderKernel::new_static(LadderCoeffs::new(
            cutoff,
            SR,
            1.0,
            4.0,
            LadderVariant::Juno60,
        ));
        let mut peak = 0.0f32;
        for n in 0..SR as usize {
            let x = if (n / 64) % 2 == 0 { 1.0 } else { -1.0 };
            let y = k.tick(x);
            assert!(y.is_finite(), "non-finite at {cutoff} Hz, n={n}");
            peak = peak.max(y.abs());
        }
        // tanh clamps post-feedback drive; output should stay in a sane range.
        assert!(peak < 4.0, "explosion at cutoff={cutoff}: peak={peak}");
    }
}

#[test]
fn self_oscillates_at_max_resonance() {
    // At k = 4 with zero input, once the state is kicked the ladder should
    // sustain oscillation rather than decay to silence.
    let mut k = LadderKernel::new_static(LadderCoeffs::new(
        1_000.0,
        SR,
        1.0,
        1.0,
        LadderVariant::Juno60,
    ));
    // Kick: one sample of input energy.
    for _ in 0..16 {
        k.tick(0.5);
    }
    // Let initial transient settle slightly.
    for _ in 0..4_000 {
        k.tick(0.0);
    }
    let mut peak = 0.0f32;
    for _ in 0..SR as usize {
        peak = peak.max(k.tick(0.0).abs());
    }
    assert!(peak > 0.05, "self-oscillation did not sustain: peak={peak}");
}

#[test]
fn variant_106_has_less_hf_than_60() {
    // Drive a near-Nyquist signal through both variants at high cutoff; 106
    // should deliver less energy thanks to the stage-gain HF trim.
    let run = |variant: LadderVariant| -> f32 {
        let mut k = LadderKernel::new_static(LadderCoeffs::new(
            10_000.0,
            SR,
            0.0,
            1.0,
            variant,
        ));
        let mut sq = 0.0f64;
        let n = 8_192;
        for i in 0..n {
            // 12 kHz tone, above most of the filter's "clean" passband.
            let x = (2.0 * PI * 12_000.0 * i as f32 / SR).sin();
            let y = k.tick(x);
            sq += (y as f64) * (y as f64);
        }
        (sq / n as f64).sqrt() as f32
    };
    let rms_60 = run(LadderVariant::Juno60);
    let rms_106 = run(LadderVariant::Juno106);
    assert!(
        rms_106 < rms_60,
        "106 should show more HF loss than 60: 60={rms_60}, 106={rms_106}"
    );
}

#[test]
fn variant_106_bass_compresses_with_resonance() {
    // At high resonance the 106 trims the input; RMS of a low tone should
    // drop relative to the 60 variant at the same resonance.
    let rms = |variant: LadderVariant, res: f32| -> f32 {
        let mut k = LadderKernel::new_static(LadderCoeffs::new(
            2_000.0,
            SR,
            res,
            1.0,
            variant,
        ));
        // Warm up to skip the transient.
        for i in 0..2_048 {
            let x = (2.0 * PI * 80.0 * i as f32 / SR).sin();
            k.tick(x);
        }
        let n = 8_192;
        let mut sq = 0.0f64;
        for i in 0..n {
            let x = (2.0 * PI * 80.0 * (i + 2_048) as f32 / SR).sin();
            let y = k.tick(x);
            sq += (y as f64) * (y as f64);
        }
        (sq / n as f64).sqrt() as f32
    };
    let ratio_60 = rms(LadderVariant::Juno60, 0.9) / rms(LadderVariant::Juno60, 0.0);
    let ratio_106 = rms(LadderVariant::Juno106, 0.9) / rms(LadderVariant::Juno106, 0.0);
    assert!(
        ratio_106 < ratio_60,
        "106 bass should compress more with resonance: 60 ratio={ratio_60}, 106 ratio={ratio_106}"
    );
}

#[test]
fn poly_matches_mono_for_same_voice() {
    let mut mono = fresh(800.0, 0.5, LadderVariant::Juno60);
    let mut poly = PolyLadderKernel::new_static(LadderCoeffs::new(
        800.0,
        SR,
        0.5,
        1.0,
        LadderVariant::Juno60,
    ));
    for i in 0..512 {
        let x = (2.0 * PI * 220.0 * i as f32 / SR).sin();
        let mut input = [0.0f32; 16];
        input[3] = x;
        let ym = mono.tick(x);
        let yp = poly.tick_all(&input, false)[3];
        assert!((ym - yp).abs() < 1.0e-5, "mono/poly mismatch at i={i}: {ym} vs {yp}");
    }
}
