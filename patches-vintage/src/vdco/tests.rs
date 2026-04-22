use super::*;
use patches_core::cables::CableKind;
use patches_core::test_support::{ModuleHarness, params};
use patches_core::{AudioEnvironment, ModuleShape};

use self::core::C0_FREQ;

fn env(sample_rate: f32) -> AudioEnvironment {
    AudioEnvironment {
        sample_rate,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

// ── Descriptor port kinds (acceptance: mono vs poly) ────────────────────────

#[test]
fn vdco_ports_are_mono() {
    let shape = ModuleShape::default();
    let d = VDco::describe(&shape);
    for name in ["voct", "pwm"] {
        let p = d.inputs.iter().find(|p| p.name == name).unwrap();
        assert_eq!(p.kind, CableKind::Mono, "VDco input '{name}' must be mono");
    }
    let out = d.outputs.iter().find(|p| p.name == "out").unwrap();
    assert_eq!(out.kind, CableKind::Mono, "VDco output must be mono");
}

#[test]
fn vpolydco_ports_are_poly() {
    let shape = ModuleShape::default();
    let d = VPolyDco::describe(&shape);
    for name in ["voct", "pwm"] {
        let p = d.inputs.iter().find(|p| p.name == name).unwrap();
        assert_eq!(p.kind, CableKind::Poly, "VPolyDco input '{name}' must be poly");
    }
    let out = d.outputs.iter().find(|p| p.name == "out").unwrap();
    assert_eq!(out.kind, CableKind::Poly, "VPolyDco output must be poly");
}

// ── Saw base pitch ──────────────────────────────────────────────────────────

/// With `sample_rate = C0 * 100` and no voct CV, saw wraps every 100 samples.
#[test]
fn saw_only_has_base_period() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params!["saw_on" => true, "pulse_on" => false],
        env(sr),
    );
    h.disconnect_all_inputs();

    let samples = h.run_mono(period * 2, "out");
    // Count wraps: sawtooth is monotone-rising then jumps down at each period.
    let mut wraps = 0usize;
    for w in samples.windows(2) {
        if w[1] < w[0] - 0.5 {
            wraps += 1;
        }
    }
    assert!(
        (1..=3).contains(&wraps),
        "expected ~2 saw wraps in 200 samples at C0; got {wraps}"
    );
}

// ── Sub = saw − 1 octave (phase-lock) ───────────────────────────────────────

/// With saw off and only the sub active, the output is a square at half the
/// base frequency: one full cycle every 2 base periods.
#[test]
fn sub_only_is_one_octave_below_saw() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params!["saw_on" => false, "pulse_on" => false, "sub_level" => 1.0_f32],
        env(sr),
    );
    h.disconnect_all_inputs();

    // Run two full sub cycles (4 * period) to capture an interior transition
    // plus the next midpoint transition.
    let samples = h.run_mono(period * 4, "out");

    // Count sign flips excluding the tiny BLEP-smoothed region.
    let mut flips = 0usize;
    let mut prev_sign = 0i32;
    for &v in &samples {
        let s = if v > 0.5 { 1 } else if v < -0.5 { -1 } else { 0 };
        if s != 0 && s != prev_sign && prev_sign != 0 {
            flips += 1;
        }
        if s != 0 {
            prev_sign = s;
        }
    }
    // ÷2 square over 4 base periods = 2 full sub cycles → 3 interior transitions.
    assert_eq!(
        flips, 3,
        "÷2 sub should flip 3× across 2 full sub cycles; got {flips}"
    );
}

/// Saw + sub at equal frequency ratio: the combined wave must be exactly
/// periodic with period = 2 * base period (no beating — perfect phase-lock).
#[test]
fn saw_plus_sub_phase_locks_no_beat() {
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params!["saw_on" => true, "pulse_on" => false, "sub_level" => 1.0_f32],
        env(sr),
    );
    h.disconnect_all_inputs();

    let n = period * 6; // 3 sub cycles
    let samples = h.run_mono(n, "out");
    // Compare cycle 1 vs cycle 2 (2 * period apart). Tolerate f32 drift.
    let sub_period = period * 2;
    let mut max_diff = 0.0_f32;
    for i in 0..sub_period {
        let d = (samples[i + sub_period] - samples[i + 2 * sub_period]).abs();
        if d > max_diff {
            max_diff = d;
        }
    }
    assert!(
        max_diff < 1e-3,
        "saw+sub not periodic at sub period (max diff {max_diff})"
    );
}

// ── PWM bit-accurate duty cycle (pulse reads raw phase, not BLEP'd saw) ─────

#[test]
fn pulse_duty_follows_pwm_cv() {
    let period = 200_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VDco>(
        params!["saw_on" => false, "pulse_on" => true],
        env(sr),
    );
    h.disconnect_input("voct");

    // pwm = 0.25 → pulse high for the first quarter of each cycle.
    h.set_mono("pwm", 0.25);
    let samples = h.run_mono(period, "out");
    let positive = samples.iter().filter(|&&v| v > 0.0).count();
    // Expected ~25% (50/200). Wider bound accounts for polyBLEP smoothing.
    assert!(
        (40..=60).contains(&positive),
        "pwm=0.25 expected ~50 positive samples; got {positive}"
    );
}

// ── VPolyDco: voice 1 runs one octave up of voice 0 ─────────────────────────

#[test]
fn poly_voct_drives_per_voice_pitch() {
    // At sr = C0 * 100, voice 0 (voct=0) saw wraps every 100 samples; voice 1
    // (voct=1, one octave up) wraps every 50 samples.
    let period = 100_usize;
    let sr = C0_FREQ * period as f32;
    let mut h = ModuleHarness::build_with_env::<VPolyDco>(
        params!["saw_on" => true],
        env(sr),
    );
    h.disconnect_input("pwm");

    let mut voct = [0.0f32; 16];
    voct[1] = 1.0;
    h.set_poly("voct", voct);

    let n = 200_usize;
    let frames = h.run_poly(n, "out");
    let (mut wraps0, mut wraps1) = (0usize, 0usize);
    for w in frames.windows(2) {
        if w[1][0] < w[0][0] - 0.5 {
            wraps0 += 1;
        }
        if w[1][1] < w[0][1] - 0.5 {
            wraps1 += 1;
        }
    }
    assert!(
        wraps1 >= 2 * wraps0.saturating_sub(1),
        "voice 1 should wrap ~2× voice 0; got v0={wraps0} v1={wraps1}"
    );
}
