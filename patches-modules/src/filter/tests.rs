use super::*;
use crate::common::approximate::fast_exp2;
use crate::common::frequency::C0_FREQ;
use patches_core::AudioEnvironment;
use patches_core::test_support::{assert_attenuated, assert_passes, ModuleHarness, params};

fn make_lowpass(cutoff_voct: f32, resonance: f32, sr: f32) -> ModuleHarness {
    let mut h = ModuleHarness::build_with_env::<ResonantLowpass>(
        params!["cutoff" => cutoff_voct, "resonance" => resonance],
        AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false },
    );
    h.disconnect_inputs(&["voct", "fm", "resonance_cv"]);
    h
}

fn make_highpass(cutoff_voct: f32, resonance: f32, sr: f32) -> ModuleHarness {
    let mut h = ModuleHarness::build_with_env::<ResonantHighpass>(
        params!["cutoff" => cutoff_voct, "resonance" => resonance],
        AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false },
    );
    h.disconnect_inputs(&["voct", "fm", "resonance_cv"]);
    h
}

fn make_bandpass(center_voct: f32, bandwidth_q: f32, sr: f32) -> ModuleHarness {
    let mut h = ModuleHarness::build_with_env::<ResonantBandpass>(
        params!["center" => center_voct, "bandwidth_q" => bandwidth_q],
        AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false },
    );
    h.disconnect_inputs(&["voct", "fm", "resonance_cv"]);
    h
}

/// Settle a filter by running `n` silent samples through it.
fn settle(h: &mut ModuleHarness, n: usize) {
    h.set_mono("in", 0.0);
    h.run_mono(n, "out");
}

/// Run a sine wave at `freq_hz` for `n` samples and return the peak absolute output.
fn measure_peak(h: &mut ModuleHarness, freq_hz: f32, sample_rate: f32, n: usize) -> f32 {
    let sine: Vec<f32> = (0..n)
        .map(|i| (TAU * freq_hz * i as f32 / sample_rate).sin())
        .collect();
    h.run_mono_mapped(n, "in", &sine, "out")
        .into_iter()
        .map(f32::abs)
        .fold(0.0_f32, f32::max)
}

// ── Lowpass tests ────────────────────────────────────────────────────────

#[test]
fn passes_dc_after_settling() {
    let mut h = make_lowpass(6.0, 0.0, 44100.0);
    let out = h.run_mono_mapped(4096, "in", &[1.0_f32], "out");
    assert!(
        (out[4095] - 1.0).abs() < 0.001,
        "DC should pass through lowpass; got {}",
        out[4095]
    );
}

#[test]
fn attenuates_above_cutoff() {
    let sr = 44100.0;
    let mut h = make_lowpass(6.0, 0.0, sr);
    settle(&mut h, 4096);
    let peak = measure_peak(&mut h, 10_000.0, sr, 1024);
    assert_attenuated!(peak, 0.05);
}

#[test]
fn resonance_boosts_near_cutoff() {
    let sr = 44100.0;
    let cutoff_voct = 6.0_f32;
    let cutoff_hz = C0_FREQ * fast_exp2(cutoff_voct); // ≈ 1047 Hz
    let mut flat = make_lowpass(cutoff_voct, 0.0, sr);
    let mut resonant = make_lowpass(cutoff_voct, 0.8, sr);
    settle(&mut flat, 4096);
    settle(&mut resonant, 4096);
    let flat_peak = measure_peak(&mut flat, cutoff_hz, sr, 4096);
    let res_peak = measure_peak(&mut resonant, cutoff_hz, sr, 4096);
    assert!(
        res_peak > flat_peak * 1.5,
        "resonance should boost signal near cutoff; flat={flat_peak}, resonant={res_peak}"
    );
}

#[test]
fn cutoff_cv_shifts_cutoff_upward() {
    let sr = 44100.0;
    // base=C5≈523 Hz; +1V→C6≈1047 Hz; test_freq sits between the two.
    let base_cutoff = 5.0_f32; // V/oct
    let test_freq = 800.0;

    let mut no_cv = make_lowpass(base_cutoff, 0.0, sr);
    let mut with_cv = ModuleHarness::build_with_env::<ResonantLowpass>(
        params!["cutoff" => base_cutoff, "resonance" => 0.0_f32],
        AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false },
    );

    // Settle with_cv with +1V/oct offset on voct.
    with_cv.set_mono("voct", 1.0);
    with_cv.set_mono("resonance_cv", 0.0);
    settle(&mut no_cv, 4096);
    settle(&mut with_cv, 4096);

    let no_cv_sine: Vec<f32> = (0..4096)
        .map(|i| (TAU * test_freq * i as f32 / sr).sin())
        .collect();
    let no_cv_peak = h_measure_peak_with_cv(&mut no_cv, &no_cv_sine, None);
    let with_cv_peak = h_measure_peak_with_cv(&mut with_cv, &no_cv_sine, None);

    assert!(
        with_cv_peak > no_cv_peak * 1.5,
        "voct +1 oct should raise cutoff (C5→C6) and reduce attenuation at {test_freq} Hz; \
         no_cv={no_cv_peak:.4}, with_cv={with_cv_peak:.4}"
    );
}

/// Helper: run a sine buffer through the harness and return peak output.
/// If `cv_override` is Some(v), set `voct` to v before each tick.
fn h_measure_peak_with_cv(
    h: &mut ModuleHarness,
    sine: &[f32],
    cv_override: Option<f32>,
) -> f32 {
    let mut peak = 0.0_f32;
    for &x in sine {
        h.set_mono("in", x);
        if let Some(cv) = cv_override {
            h.set_mono("voct", cv);
        }
        h.tick();
        peak = peak.max(h.read_mono("out").abs());
    }
    peak
}

#[test]
fn static_path_passes_dc_when_no_cv() {
    let mut h = make_lowpass(6.0, 0.0, 44100.0);
    let out = h.run_mono_mapped(4096, "in", &[1.0_f32], "out");
    assert!(
        (out[4095] - 1.0).abs() < 0.001,
        "DC should pass in static path; got {}",
        out[4095]
    );
}

// ── Highpass tests ────────────────────────────────────────────────────────

#[test]
fn highpass_attenuates_below_cutoff() {
    let sr = 44100.0;
    let mut h = make_highpass(6.0, 0.0, sr);
    settle(&mut h, 4096);
    let peak = measure_peak(&mut h, 100.0, sr, 4096);
    assert_attenuated!(peak, 0.05);
}

#[test]
fn highpass_passes_above_cutoff() {
    let sr = 44100.0;
    let mut h = make_highpass(6.0, 0.0, sr);
    settle(&mut h, 4096);
    let peak = measure_peak(&mut h, 11025.0, sr, 4096);
    assert_passes!(peak, 0.9);
}

#[test]
fn highpass_resonance_boosts_near_cutoff() {
    let sr = 44100.0;
    let cutoff_voct = 6.0_f32;
    let cutoff_hz = C0_FREQ * fast_exp2(cutoff_voct); // ≈ 1047 Hz
    let mut flat = make_highpass(cutoff_voct, 0.0, sr);
    let mut resonant = make_highpass(cutoff_voct, 0.8, sr);
    settle(&mut flat, 4096);
    settle(&mut resonant, 4096);
    let flat_peak = measure_peak(&mut flat, cutoff_hz, sr, 4096);
    let res_peak = measure_peak(&mut resonant, cutoff_hz, sr, 4096);
    assert!(
        res_peak > flat_peak * 1.5,
        "resonance should boost signal near cutoff; flat={flat_peak}, resonant={res_peak}"
    );
}

#[test]
fn highpass_cutoff_cv_shifts_cutoff() {
    // +1 V/oct raises the cutoff one octave (C5≈523 Hz → C6≈1047 Hz). A
    // test signal at 800 Hz — above the base cutoff but below the raised
    // cutoff — should experience more attenuation when CV is applied.
    let sr = 44100.0;
    let base_cutoff = 5.0_f32; // V/oct (C5 ≈ 523 Hz)
    let test_freq = 800.0;

    let mut no_cv = make_highpass(base_cutoff, 0.0, sr);
    let mut with_cv = ModuleHarness::build_with_env::<ResonantHighpass>(
        params!["cutoff" => base_cutoff, "resonance" => 0.0_f32],
        AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false },
    );
    with_cv.set_mono("voct", 1.0);
    with_cv.set_mono("resonance_cv", 0.0);

    settle(&mut no_cv, 4096);
    settle(&mut with_cv, 4096);

    let sine: Vec<f32> = (0..4096)
        .map(|i| (TAU * test_freq * i as f32 / sr).sin())
        .collect();
    let no_cv_peak  = h_measure_peak_with_cv(&mut no_cv,  &sine, None);
    let with_cv_peak = h_measure_peak_with_cv(&mut with_cv, &sine, None);

    // Without CV (cutoff=500 Hz): test_freq (800 Hz) is in the passband → passes.
    // With +1V (cutoff=1000 Hz): test_freq is now in the stop-band → attenuated.
    assert!(
        no_cv_peak > with_cv_peak * 1.5,
        "voct +1 oct should raise cutoff (C5→C6) and increase attenuation at {test_freq} Hz; \
         no_cv={no_cv_peak:.4}, with_cv={with_cv_peak:.4}"
    );
}

// ── Bandpass tests ────────────────────────────────────────────────────────

#[test]
fn bandpass_attenuates_far_below_center() {
    let sr = 44100.0;
    let mut h = make_bandpass(6.0, 3.0, sr);
    settle(&mut h, 4096);
    let peak = measure_peak(&mut h, 100.0, sr, 4096);
    assert_attenuated!(peak, 0.1);
}

#[test]
fn bandpass_attenuates_far_above_center() {
    let sr = 44100.0;
    let mut h = make_bandpass(6.0, 3.0, sr);
    settle(&mut h, 4096);
    let peak = measure_peak(&mut h, 10_000.0, sr, 4096);
    assert_attenuated!(peak, 0.1);
}

#[test]
fn bandpass_passes_at_center() {
    let sr = 44100.0;
    let center_voct = 6.0_f32;
    let center_hz = C0_FREQ * fast_exp2(center_voct); // ≈ 1047 Hz
    let mut h = make_bandpass(center_voct, 1.0, sr);
    settle(&mut h, 4096);
    let peak = measure_peak(&mut h, center_hz, sr, 4096);
    assert_passes!(peak, 0.8);
}

#[test]
fn bandpass_narrow_q_is_narrower_than_wide_q() {
    let sr = 44100.0;
    let center_voct = 6.0_f32; // ≈ 1047 Hz
    let test_freq = 2000.0;    // 1 octave above centre
    let mut narrow = make_bandpass(center_voct, 10.0, sr);
    let mut wide = make_bandpass(center_voct, 0.5, sr);
    settle(&mut narrow, 4096);
    settle(&mut wide, 4096);
    let narrow_peak = measure_peak(&mut narrow, test_freq, sr, 4096);
    let wide_peak = measure_peak(&mut wide, test_freq, sr, 4096);
    assert!(
        narrow_peak < wide_peak,
        "narrow Q (10) should attenuate more at 1 oct off-centre than wide Q (0.5); \
         narrow={narrow_peak:.4}, wide={wide_peak:.4}"
    );
}

#[test]
fn bandpass_center_cv_shifts_center() {
    // +1 V/oct raises the centre one octave (C6≈1047 Hz → C7≈2093 Hz). A
    // test signal at 2000 Hz is in the stop-band without CV but near the
    // new centre with +1V applied.
    let sr = 44100.0;
    let base_center = 6.0_f32; // V/oct (C6 ≈ 1047 Hz)
    let test_freq = 2000.0;

    // Q=3: narrow enough that 2000 Hz is well outside the C6 passband.
    let mut no_cv = make_bandpass(base_center, 3.0, sr);
    let mut with_cv = ModuleHarness::build_with_env::<ResonantBandpass>(
        params!["center" => base_center, "bandwidth_q" => 3.0_f32],
        AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false },
    );
    with_cv.set_mono("voct", 1.0);
    with_cv.set_mono("resonance_cv", 0.0);

    settle(&mut no_cv, 4096);
    settle(&mut with_cv, 4096);

    let sine: Vec<f32> = (0..4096)
        .map(|i| (TAU * test_freq * i as f32 / sr).sin())
        .collect();
    let no_cv_peak  = h_measure_peak_with_cv(&mut no_cv,  &sine, None);
    let with_cv_peak = h_measure_peak_with_cv(&mut with_cv, &sine, None);

    assert!(
        with_cv_peak > no_cv_peak * 1.5,
        "voct +1 oct should shift centre (C6→C7) and increase output at {test_freq} Hz; \
         no_cv={no_cv_peak:.4}, with_cv={with_cv_peak:.4}"
    );
}
