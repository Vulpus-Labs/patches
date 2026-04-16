use std::f32::consts::TAU;

use super::*;
use crate::common::frequency::C0_FREQ;
use patches_core::{
    AudioEnvironment, CablePool, CableValue, InstanceId, Module, ModuleShape,
    PolyInput, PolyOutput, Registry, COEFF_UPDATE_INTERVAL,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::test_support::{assert_attenuated, assert_passes};

// ── helpers ──────────────────────────────────────────────────────────

fn make_poly_pool(n: usize) -> Vec<[CableValue; 2]> {
    vec![[CableValue::Poly([0.0; 16]); 2]; n]
}

fn make_lowpass_sr(cutoff_voct: f32, resonance: f32, sr: f32) -> Box<dyn Module> {
    let mut params = ParameterMap::new();
    params.insert("cutoff".into(), ParameterValue::Float(cutoff_voct));
    params.insert("resonance".into(), ParameterValue::Float(resonance));
    let mut r = Registry::new();
    r.register::<PolyResonantLowpass>();
    r.create(
        "PolyLowpass",
        &AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false },
        &ModuleShape { channels: 0, length: 0, ..Default::default() },
        &params,
        InstanceId::next(),
    )
    .unwrap()
}

fn make_highpass_sr(cutoff_voct: f32, resonance: f32, sr: f32) -> Box<dyn Module> {
    let mut params = ParameterMap::new();
    params.insert("cutoff".into(), ParameterValue::Float(cutoff_voct));
    params.insert("resonance".into(), ParameterValue::Float(resonance));
    let mut r = Registry::new();
    r.register::<PolyResonantHighpass>();
    r.create(
        "PolyHighpass",
        &AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false },
        &ModuleShape { channels: 0, length: 0, ..Default::default() },
        &params,
        InstanceId::next(),
    )
    .unwrap()
}

fn make_bandpass_sr(center_voct: f32, bandwidth_q: f32, sr: f32) -> Box<dyn Module> {
    let mut params = ParameterMap::new();
    params.insert("center".into(), ParameterValue::Float(center_voct));
    params.insert("bandwidth_q".into(), ParameterValue::Float(bandwidth_q));
    let mut r = Registry::new();
    r.register::<PolyResonantBandpass>();
    r.create(
        "PolyBandpass",
        &AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32, hosted: false },
        &ModuleShape { channels: 0, length: 0, ..Default::default() },
        &params,
        InstanceId::next(),
    )
    .unwrap()
}

/// Set static ports: in=connected, voct=disconnected, fm=disconnected, resonance_cv=disconnected.
/// Pool layout: 0=in_audio, 1=voct, 2=fm, 3=resonance_cv, 4=out_audio.
fn set_static_ports(m: &mut Box<dyn Module>) {
    m.set_ports(
        &[
            InputPort::Poly(PolyInput { cable_idx: 0, scale: 1.0, connected: true }),
            InputPort::Poly(PolyInput { cable_idx: 1, scale: 1.0, connected: false }),
            InputPort::Poly(PolyInput { cable_idx: 2, scale: 1.0, connected: false }),
            InputPort::Poly(PolyInput { cable_idx: 3, scale: 1.0, connected: false }),
        ],
        &[OutputPort::Poly(PolyOutput { cable_idx: 4, connected: true })],
    );
}

/// Set CV ports: in=connected, voct=connected, fm=disconnected, resonance_cv=disconnected.
fn set_cutoff_cv_ports(m: &mut Box<dyn Module>) {
    m.set_ports(
        &[
            InputPort::Poly(PolyInput { cable_idx: 0, scale: 1.0, connected: true }),
            InputPort::Poly(PolyInput { cable_idx: 1, scale: 1.0, connected: true }),
            InputPort::Poly(PolyInput { cable_idx: 2, scale: 1.0, connected: false }),
            InputPort::Poly(PolyInput { cable_idx: 3, scale: 1.0, connected: false }),
        ],
        &[OutputPort::Poly(PolyOutput { cable_idx: 4, connected: true })],
    );
}

/// Set center CV ports for bandpass: in=connected, voct=connected, fm=disconnected, resonance_cv=disconnected.
fn set_center_cv_ports(m: &mut Box<dyn Module>) {
    set_cutoff_cv_ports(m);
}

fn settle(m: &mut Box<dyn Module>, n: usize) {
    let mut pool = make_poly_pool(5);
    for i in 0..n {
        let wi = i % 2;
        pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
        m.process(&mut CablePool::new(&mut pool, wi));
    }
}

/// Feed a sine at `freq_hz` (same value in all 16 channels) and return per-voice peak.
fn measure_peak_all_voices(
    m: &mut Box<dyn Module>,
    freq_hz: f32,
    sr: f32,
    n: usize,
) -> [f32; 16] {
    let mut pool = make_poly_pool(5);
    let mut peaks = [0.0f32; 16];
    for i in 0..n {
        let wi = i % 2;
        let x = (TAU * freq_hz * i as f32 / sr).sin();
        pool[0][1 - wi] = CableValue::Poly([x; 16]);
        m.process(&mut CablePool::new(&mut pool, wi));
        if let CableValue::Poly(v) = pool[4][wi] {
            for j in 0..16 {
                peaks[j] = peaks[j].max(v[j].abs());
            }
        }
    }
    peaks
}

// ── PolyResonantLowpass tests ─────────────────────────────────────────

#[test]
fn poly_lowpass_all_voices_pass_dc() {
    let sr = 44100.0;
    let mut f = make_lowpass_sr(6.0, 0.0, sr);
    set_static_ports(&mut f);
    let mut pool = make_poly_pool(5);
    // 4096 silent samples
    for i in 0..4096 {
        let wi = i % 2;
        pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
        f.process(&mut CablePool::new(&mut pool, wi));
    }
    // 4096 DC samples
    for i in 0..4096 {
        let wi = i % 2;
        pool[0][1 - wi] = CableValue::Poly([1.0; 16]);
        f.process(&mut CablePool::new(&mut pool, wi));
    }
    if let CableValue::Poly(v) = pool[4][(4095) % 2] {
        for (i, &ch) in v.iter().enumerate() {
            assert!(
                (ch - 1.0).abs() < 0.01,
                "voice {i}: DC should pass through lowpass; got {ch}"
            );
        }
    } else {
        panic!("expected Poly output");
    }
}

#[test]
fn poly_lowpass_all_voices_attenuate_above_cutoff() {
    let sr = 44100.0;
    let mut f = make_lowpass_sr(6.0, 0.0, sr);
    set_static_ports(&mut f);
    settle(&mut f, 4096);
    let peaks = measure_peak_all_voices(&mut f, 10_000.0, sr, 1024);
    for (i, &p) in peaks.iter().enumerate() {
        assert_attenuated!(p, 0.05, "voice {i}: expected attenuation above cutoff; peak={p}");
    }
}

#[test]
fn poly_lowpass_voices_are_independent_with_cv() {
    let sr = 44100.0;
    let base_cutoff = 5.0_f32; // V/oct (C5 ≈ 523 Hz)
    let test_freq = 700.0;

    let mut f = make_lowpass_sr(base_cutoff, 0.0, sr);
    set_cutoff_cv_ports(&mut f);

    // CV array: voice 0 gets +1 V/oct (cutoff→C6≈1047 Hz, test_freq passes better),
    //            voice 15 gets -2 V/oct (cutoff→C3≈130 Hz, test_freq strongly attenuated).
    let mut cv = [0.0f32; 16];
    cv[0] = 1.0;
    cv[15] = -2.0;

    let mut pool = make_poly_pool(5);
    // Settle with CV applied
    for i in 0..4096 {
        let wi = i % 2;
        pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
        pool[1][1 - wi] = CableValue::Poly(cv);
        if i % COEFF_UPDATE_INTERVAL as usize == 0 {
            if let Some(p) = f.as_periodic() {
                p.periodic_update(&CablePool::new(&mut pool, wi));
            }
        }
        f.process(&mut CablePool::new(&mut pool, wi));
    }
    // Measure peaks with CV applied
    let mut peaks = [0.0f32; 16];
    for i in 0..4096usize {
        let wi = i % 2;
        let x = (TAU * test_freq * i as f32 / sr).sin();
        pool[0][1 - wi] = CableValue::Poly([x; 16]);
        pool[1][1 - wi] = CableValue::Poly(cv);
        if i % COEFF_UPDATE_INTERVAL as usize == 0 {
            if let Some(p) = f.as_periodic() {
                p.periodic_update(&CablePool::new(&mut pool, wi));
            }
        }
        f.process(&mut CablePool::new(&mut pool, wi));
        if let CableValue::Poly(v) = pool[4][wi] {
            for j in 0..16 {
                peaks[j] = peaks[j].max(v[j].abs());
            }
        }
    }
    assert!(
        peaks[0] > peaks[15] * 2.0,
        "voice 0 (cutoff→C6≈1047 Hz) should pass {test_freq} Hz more than voice 15 (cutoff→C3≈130 Hz); \
         voice0={:.4}, voice15={:.4}", peaks[0], peaks[15]
    );
}

#[test]
fn poly_lowpass_static_path_when_no_cv() {
    let sr = 44100.0;
    let mut f = make_lowpass_sr(6.0, 0.0, sr);
    set_static_ports(&mut f);
    let mut pool = make_poly_pool(5);
    for i in 0..100 {
        let wi = i % 2;
        pool[0][1 - wi] = CableValue::Poly([0.5; 16]);
        f.process(&mut CablePool::new(&mut pool, wi));
    }
    // Downcast to inspect internal state: all deltas should be zero in static path.
    let concrete = f.as_any().downcast_ref::<PolyResonantLowpass>().unwrap();
    for i in 0..16 {
        assert_eq!(concrete.biquad.db0[i], 0.0, "voice {i}: db0 should be zero in static path");
        assert_eq!(concrete.biquad.db1[i], 0.0, "voice {i}: db1 should be zero in static path");
        assert_eq!(concrete.biquad.db2[i], 0.0, "voice {i}: db2 should be zero in static path");
        assert_eq!(concrete.biquad.da1[i], 0.0, "voice {i}: da1 should be zero in static path");
        assert_eq!(concrete.biquad.da2[i], 0.0, "voice {i}: da2 should be zero in static path");
    }
}

// ── PolyResonantHighpass tests ────────────────────────────────────────

#[test]
fn poly_highpass_attenuates_below_cutoff() {
    let sr = 44100.0;
    let mut f = make_highpass_sr(6.0, 0.0, sr);
    set_static_ports(&mut f);
    settle(&mut f, 4096);
    let peaks = measure_peak_all_voices(&mut f, 100.0, sr, 4096);
    for (i, &p) in peaks.iter().enumerate() {
        assert_attenuated!(p, 0.05, "voice {i}: expected attenuation at cutoff/10; peak={p}");
    }
}

#[test]
fn poly_highpass_passes_above_cutoff() {
    let sr = 44100.0;
    let mut f = make_highpass_sr(6.0, 0.0, sr);
    set_static_ports(&mut f);
    settle(&mut f, 4096);
    // Nyquist/2 ≈ 11025 Hz — well into the highpass passband.
    let peaks = measure_peak_all_voices(&mut f, 11025.0, sr, 4096);
    for (i, &p) in peaks.iter().enumerate() {
        assert_passes!(p, 0.9, "voice {i}: expected near-unity gain above cutoff; peak={p}");
    }
}

#[test]
fn poly_highpass_voices_are_independent_with_cv() {
    // +1 V/oct raises the cutoff one octave (C5≈523 Hz → C6≈1047 Hz).
    // Test signal at 800 Hz: above the base cutoff but below the raised cutoff.
    // Voice 0 gets +1 V/oct → cutoff≈1047 Hz → 800 Hz is attenuated.
    // Voice 15 gets no CV → cutoff≈523 Hz → 800 Hz passes.
    let sr = 44100.0;
    let base_cutoff = 5.0_f32; // V/oct (C5 ≈ 523 Hz)
    let test_freq = 800.0;

    let mut f = make_highpass_sr(base_cutoff, 0.0, sr);
    set_cutoff_cv_ports(&mut f);

    let mut cv = [0.0f32; 16];
    cv[0] = 1.0; // voice 0: cutoff→C6≈1047 Hz, test_freq now in stop-band

    let mut pool = make_poly_pool(5);
    for i in 0..4096 {
        let wi = i % 2;
        pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
        pool[1][1 - wi] = CableValue::Poly(cv);
        if i % COEFF_UPDATE_INTERVAL as usize == 0 {
            if let Some(p) = f.as_periodic() {
                p.periodic_update(&CablePool::new(&mut pool, wi));
            }
        }
        f.process(&mut CablePool::new(&mut pool, wi));
    }
    let mut peaks = [0.0f32; 16];
    for i in 0..4096usize {
        let wi = i % 2;
        let x = (TAU * test_freq * i as f32 / sr).sin();
        pool[0][1 - wi] = CableValue::Poly([x; 16]);
        pool[1][1 - wi] = CableValue::Poly(cv);
        if i % COEFF_UPDATE_INTERVAL as usize == 0 {
            if let Some(p) = f.as_periodic() {
                p.periodic_update(&CablePool::new(&mut pool, wi));
            }
        }
        f.process(&mut CablePool::new(&mut pool, wi));
        if let CableValue::Poly(v) = pool[4][wi] {
            for j in 0..16 {
                peaks[j] = peaks[j].max(v[j].abs());
            }
        }
    }
    // Voice 15 (cutoff=C5≈523 Hz): 800 Hz is in the passband → larger peak.
    // Voice 0 (cutoff=C6≈1047 Hz): 800 Hz is in the stop-band → smaller peak.
    assert!(
        peaks[15] > peaks[0] * 1.5,
        "voice 15 (cutoff=C5≈523 Hz) should pass {test_freq} Hz more than voice 0 (cutoff=C6≈1047 Hz); \
         voice15={:.4}, voice0={:.4}", peaks[15], peaks[0]
    );
}

// ── PolyResonantBandpass tests ────────────────────────────────────────

#[test]
fn poly_bandpass_attenuates_far_from_center() {
    let sr = 44100.0;
    // Q=3: narrow enough that ±1 octave is well outside the passband.
    let mut f = make_bandpass_sr(6.0, 3.0, sr);
    set_static_ports(&mut f);
    settle(&mut f, 4096);
    let peaks_low = measure_peak_all_voices(&mut f, 100.0, sr, 4096);
    settle(&mut f, 4096);
    let peaks_high = measure_peak_all_voices(&mut f, 10_000.0, sr, 4096);
    for (i, (&pl, &ph)) in peaks_low.iter().zip(peaks_high.iter()).enumerate() {
        assert_attenuated!(pl, 0.1, "voice {i}: expected attenuation at center/10; peak_low={pl}");
        assert_attenuated!(ph, 0.1, "voice {i}: expected attenuation at center×10; peak_high={ph}");
    }
}

#[test]
fn poly_bandpass_passes_at_center() {
    let sr = 44100.0;
    let center_voct = 6.0_f32;
    let center_hz = C0_FREQ * center_voct.exp2(); // ≈ 1047 Hz
    let mut f = make_bandpass_sr(center_voct, 1.0, sr);
    set_static_ports(&mut f);
    settle(&mut f, 4096);
    let peaks = measure_peak_all_voices(&mut f, center_hz, sr, 4096);
    for (i, &p) in peaks.iter().enumerate() {
        assert_passes!(p, 0.8, "voice {i}: expected near-unity gain at centre; peak={p}");
    }
}

#[test]
fn poly_bandpass_narrow_q_is_narrower_than_wide_q() {
    let sr = 44100.0;
    let center_voct = 6.0_f32; // ≈ 1047 Hz
    let test_freq = 2000.0; // one octave above center
    let mut narrow = make_bandpass_sr(center_voct, 8.0, sr);
    let mut wide = make_bandpass_sr(center_voct, 0.5, sr);
    set_static_ports(&mut narrow);
    set_static_ports(&mut wide);
    settle(&mut narrow, 4096);
    settle(&mut wide, 4096);
    let narrow_peaks = measure_peak_all_voices(&mut narrow, test_freq, sr, 4096);
    let wide_peaks = measure_peak_all_voices(&mut wide, test_freq, sr, 4096);
    for (i, (&np, &wp)) in narrow_peaks.iter().zip(wide_peaks.iter()).enumerate() {
        assert!(
            np < wp,
            "voice {i}: narrow Q=8 should attenuate more at 1 oct off-centre than Q=0.5; \
             narrow={np:.4}, wide={wp:.4}"
        );
    }
}

#[test]
fn poly_bandpass_voices_are_independent_with_cv() {
    // +1 V/oct raises the centre one octave (C6≈1047 Hz → C7≈2093 Hz). Q=3.
    // Voice 0 gets +1 V → centre≈2093 Hz → test_freq=2000 Hz is near centre → passes.
    // Voice 15 gets no CV → centre≈1047 Hz → test_freq=2000 Hz is off-centre → attenuated.
    let sr = 44100.0;
    let base_center = 6.0_f32; // V/oct (C6 ≈ 1047 Hz)
    let test_freq = 2000.0;

    let mut f = make_bandpass_sr(base_center, 3.0, sr);
    set_center_cv_ports(&mut f);

    let mut cv = [0.0f32; 16];
    cv[0] = 1.0; // voice 0: centre→C7≈2093 Hz

    let mut pool = make_poly_pool(5);
    for i in 0..4096 {
        let wi = i % 2;
        pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
        pool[1][1 - wi] = CableValue::Poly(cv);
        if i % COEFF_UPDATE_INTERVAL as usize == 0 {
            if let Some(p) = f.as_periodic() {
                p.periodic_update(&CablePool::new(&mut pool, wi));
            }
        }
        f.process(&mut CablePool::new(&mut pool, wi));
    }
    let mut peaks = [0.0f32; 16];
    for i in 0..4096usize {
        let wi = i % 2;
        let x = (TAU * test_freq * i as f32 / sr).sin();
        pool[0][1 - wi] = CableValue::Poly([x; 16]);
        pool[1][1 - wi] = CableValue::Poly(cv);
        if i % COEFF_UPDATE_INTERVAL as usize == 0 {
            if let Some(p) = f.as_periodic() {
                p.periodic_update(&CablePool::new(&mut pool, wi));
            }
        }
        f.process(&mut CablePool::new(&mut pool, wi));
        if let CableValue::Poly(v) = pool[4][wi] {
            for j in 0..16 {
                peaks[j] = peaks[j].max(v[j].abs());
            }
        }
    }
    assert!(
        peaks[0] > peaks[15] * 2.0,
        "voice 0 (centre→C7≈2093 Hz) should pass {test_freq} Hz more than voice 15 (centre=C6≈1047 Hz); \
         voice0={:.4}, voice15={:.4}", peaks[0], peaks[15]
    );
}
