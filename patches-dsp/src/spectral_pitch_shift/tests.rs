use super::*;
use crate::test_support::assert_within;

#[test]
fn principal_argument_wraps_correctly() {
    assert_within!(0.0, principal_argument(0.0), 1e-6);
    // PI and -PI are equivalent; principal_argument may return either.
    assert_within!(PI, principal_argument(PI).abs(), 1e-5);
    assert_within!(PI, principal_argument(3.0 * PI).abs(), 1e-5);
    assert_within!(PI, principal_argument(-3.0 * PI).abs(), 1e-5);
    // Small values pass through unchanged.
    assert_within!(1.0, principal_argument(1.0), 1e-6);
    assert_within!(-1.0, principal_argument(-1.0), 1e-6);
}

#[test]
fn lerp_basic() {
    let data = [0.0f32, 1.0, 2.0, 3.0];
    assert_within!(0.0, lerp(&data, 0.0), 1e-6);
    assert_within!(0.5, lerp(&data, 0.5), 1e-6);
    assert_within!(2.5, lerp(&data, 2.5), 1e-6);
    // Clamp at end.
    assert_within!(3.0, lerp(&data, 10.0), 1e-6);
}

#[test]
fn identity_shift_preserves_spectrum() {
    let window_size = 64;
    let hop_size = 16;
    let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
    shifter.set_shift_ratio(1.0);

    // Single peak at bin 4 — no DC energy to avoid edge-case
    // interactions with region boundaries.
    let mut spectrum = vec![0.0f32; window_size];
    spectrum[8] = 0.5; // bin 4 real
    spectrum[9] = 0.3; // bin 4 imag

    let original = spectrum.clone();
    shifter.transform(&mut spectrum);

    // With ratio=1.0, delta=0 for every peak, and the complex rotation
    // is a multiple of 2π on the first frame → output ≈ input.
    for (i, (&a, &b)) in original.iter().zip(spectrum.iter()).enumerate() {
        assert_within!(a, b, 1e-3, "bin {i}: expected {a}, got {b}");
    }
}

#[test]
fn octave_up_shifts_bins() {
    let window_size = 64;
    let hop_size = 16;
    let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
    shifter.set_shift_semitones(12.0); // octave up → ratio = 2.0

    // Put energy at bin 4 only.
    let mut spectrum = vec![0.0f32; window_size];
    spectrum[8] = 1.0; // bin 4 real

    shifter.transform(&mut spectrum);

    // Bin 8 (= 4 * 2) should now have energy.
    let mag_8 = spectrum[16].hypot(spectrum[17]);
    // Bin 4 should be near zero (shifted away).
    let mag_4 = spectrum[8].hypot(spectrum[9]);
    assert!(
        mag_8 > 0.5,
        "bin 8 should have energy after octave-up: {mag_8}"
    );
    assert!(
        mag_4 < 0.1,
        "bin 4 should be mostly empty after octave-up: {mag_4}"
    );
}

#[test]
fn mix_blends_dry_wet() {
    let window_size = 64;
    let hop_size = 16;
    let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
    shifter.set_shift_semitones(12.0);
    shifter.set_mix(0.0); // fully dry

    let mut spectrum = vec![0.0f32; window_size];
    spectrum[8] = 1.0;
    let original = spectrum.clone();

    shifter.transform(&mut spectrum);

    // mix=0 → output should equal original.
    for (i, (&a, &b)) in original.iter().zip(spectrum.iter()).enumerate() {
        assert_within!(a, b, 1e-6, "mix=0 bin {i}: expected {a}, got {b}");
    }
}

#[test]
fn region_preserves_phase_coherence() {
    // A peak with sidelobes shifted by a fifth: all bins in the region
    // get the same complex rotation, so inter-bin phase relationships
    // from the analysis are preserved in the output.
    let window_size = 128;
    let hop_size = 32;
    let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
    shifter.set_shift_semitones(7.0); // perfect fifth, ratio ≈ 1.498
    shifter.set_mono(true);

    // Simulate a windowed sinusoid: peak at bin 10 with sidelobes.
    let mut spectrum = vec![0.0f32; window_size];
    // Bin 10: magnitude 1.0, phase 0.3
    spectrum[20] = 0.3f32.cos(); // re
    spectrum[21] = 0.3f32.sin(); // im
    // Bin 9: magnitude 0.3, phase 0.5
    spectrum[18] = 0.3 * 0.5f32.cos();
    spectrum[19] = 0.3 * 0.5f32.sin();
    // Bin 11: magnitude 0.3, phase -0.2
    spectrum[22] = 0.3 * (-0.2f32).cos();
    spectrum[23] = 0.3 * (-0.2f32).sin();

    // Input phase differences between sidelobes and peak.
    let input_diff_9_10 = principal_argument(0.5 - 0.3);
    let input_diff_11_10 = principal_argument(-0.2 - 0.3);

    // Run a frame.
    shifter.transform(&mut spectrum);

    // Target bin ≈ round(10 * 1.498) = 15.  Region shifts by +5.
    // Bins 9,10,11 → bins 14,15,16.
    let phase_of = |bin: usize| -> f32 {
        spectrum[2 * bin + 1].atan2(spectrum[2 * bin])
    };

    let p15 = phase_of(15);
    let p14 = phase_of(14);
    let p16 = phase_of(16);

    // The rotation is the same for all bins in the region, so the
    // output inter-bin phase differences should equal the input ones.
    let output_diff_14_15 = principal_argument(p14 - p15);
    let output_diff_16_15 = principal_argument(p16 - p15);

    assert_within!(
        input_diff_9_10, output_diff_14_15, 1e-4,
        "phase diff 14-15 should match input diff 9-10"
    );
    assert_within!(
        input_diff_11_10, output_diff_16_15, 1e-4,
        "phase diff 16-15 should match input diff 11-10"
    );
}

#[test]
fn reset_clears_phase_state() {
    let mut shifter = SpectralPitchShifter::new(64, 16);
    shifter.set_shift_semitones(7.0);

    let mut spectrum = vec![0.0f32; 64];
    spectrum[8] = 1.0;

    // Test poly mode (per-bin accumulator).
    shifter.transform(&mut spectrum);
    assert!(shifter.phase_accumulator.iter().any(|&p| p != 0.0));

    shifter.reset();
    assert!(shifter.phase_accumulator.iter().all(|&p| p == 0.0));
    assert!(shifter.prev_phase.iter().all(|&p| p == 0.0));

    // Test mono mode (synth_phase).
    shifter.set_mono(true);
    spectrum.fill(0.0);
    spectrum[8] = 1.0;
    shifter.transform(&mut spectrum);
    assert!(shifter.synth_phase.iter().any(|&p| p != 0.0));

    shifter.reset();
    assert!(shifter.synth_phase.iter().all(|&p| p == 0.0));
}

// ── End-to-end audio tests (T-0260) ────────────────────────────────────

/// Helper: generate audio, window, FFT, transform, IFFT, overlap-add.
/// Returns the reconstructed output signal.
fn pitch_shift_audio(
    signal: &[f32],
    window_size: usize,
    overlap: usize,
    semitones: f32,
    mix: f32,
) -> Vec<f32> {
    use crate::fft::RealPackedFft;

    let hop = window_size / overlap;
    let fft = RealPackedFft::new(window_size);
    let mut shifter = SpectralPitchShifter::new(window_size, hop);
    shifter.set_shift_semitones(semitones);
    shifter.set_mix(mix);

    // Hann window
    let hann: Vec<f32> = (0..window_size)
        .map(|i| {
            let n = i as f32 / window_size as f32;
            0.5 * (1.0 - (2.0 * PI * n).cos())
        })
        .collect();

    let out_len = signal.len();
    let mut output = vec![0.0f32; out_len];
    let mut norm = vec![0.0f32; out_len];

    let mut pos = 0isize;
    while (pos as usize) + window_size <= signal.len() + window_size {
        let mut frame = vec![0.0f32; window_size];
        for i in 0..window_size {
            let idx = pos + i as isize;
            if idx >= 0 && (idx as usize) < signal.len() {
                frame[i] = signal[idx as usize] * hann[i];
            }
        }

        fft.forward(&mut frame);
        shifter.transform(&mut frame);
        fft.inverse(&mut frame);

        // Overlap-add with synthesis window
        for i in 0..window_size {
            let idx = pos + i as isize;
            if idx >= 0 && (idx as usize) < out_len {
                let oi = idx as usize;
                output[oi] += frame[i] * hann[i];
                norm[oi] += hann[i] * hann[i];
            }
        }

        pos += hop as isize;
    }

    // Normalise by WOLA factor
    for i in 0..out_len {
        if norm[i] > 1e-10 {
            output[i] /= norm[i];
        }
    }
    output
}

/// 440 Hz sine shifted +12 semitones should produce ~880 Hz.
#[test]
fn pitch_shift_octave_up_audio() {
    use crate::fft::RealPackedFft;
    use crate::test_support::dominant_bin;

    let sample_rate = 48_000.0;
    let window_size = 1024;
    let overlap = 4;
    let duration = 8192;

    let signal: Vec<f32> = (0..duration)
        .map(|i| (2.0 * PI * 440.0 / sample_rate * i as f32).sin())
        .collect();

    let output = pitch_shift_audio(&signal, window_size, overlap, 12.0, 1.0);

    // Analyse output with FFT — skip transient at start
    let analysis_start = window_size * 2;
    let fft_size = 2048;
    let fft = RealPackedFft::new(fft_size);
    let mut buf = vec![0.0f32; fft_size];
    let copy_len = fft_size.min(output.len() - analysis_start);
    buf[..copy_len].copy_from_slice(&output[analysis_start..analysis_start + copy_len]);
    fft.forward(&mut buf);

    let peak = dominant_bin(&buf, fft_size);

    let expected_bin = (880.0 * fft_size as f32 / sample_rate).round() as usize;
    let bin_diff = (peak as isize - expected_bin as isize).unsigned_abs();
    assert!(
        bin_diff <= 2,
        "octave up: peak at bin {peak} (expected ~{expected_bin}, 880 Hz)"
    );
}

/// Identity shift (0 semitones) should preserve the signal.
#[test]
fn pitch_shift_identity_audio() {
    let sample_rate = 48_000.0;
    let window_size = 1024;
    let overlap = 4;
    let duration = 8192;

    let signal: Vec<f32> = (0..duration)
        .map(|i| (2.0 * PI * 440.0 / sample_rate * i as f32).sin())
        .collect();

    let output = pitch_shift_audio(&signal, window_size, overlap, 0.0, 1.0);

    // Compare steady-state region (skip transients)
    let start = window_size * 2;
    let end = duration - window_size;
    let mut sum_sq_signal = 0.0f64;
    let mut sum_sq_error = 0.0f64;
    for i in start..end {
        let s = signal[i] as f64;
        let e = (output[i] - signal[i]) as f64;
        sum_sq_signal += s * s;
        sum_sq_error += e * e;
    }
    let rms_signal = (sum_sq_signal / (end - start) as f64).sqrt();
    let rms_error = (sum_sq_error / (end - start) as f64).sqrt();
    let error_ratio = rms_error / rms_signal;
    assert!(
        error_ratio < 0.1,
        "identity shift error ratio {error_ratio:.4} should be < 0.1"
    );
}

/// Mix=0.0 should return the original signal.
#[test]
fn pitch_shift_mix_zero_audio() {
    let sample_rate = 48_000.0;
    let window_size = 1024;
    let overlap = 4;
    let duration = 8192;

    let signal: Vec<f32> = (0..duration)
        .map(|i| (2.0 * PI * 440.0 / sample_rate * i as f32).sin())
        .collect();

    let output = pitch_shift_audio(&signal, window_size, overlap, 12.0, 0.0);

    // With mix=0, output should match input in the steady-state region
    let start = window_size * 2;
    let end = duration - window_size;
    let mut sum_sq_signal = 0.0f64;
    let mut sum_sq_error = 0.0f64;
    for i in start..end {
        let s = signal[i] as f64;
        let e = (output[i] - signal[i]) as f64;
        sum_sq_signal += s * s;
        sum_sq_error += e * e;
    }
    let rms_signal = (sum_sq_signal / (end - start) as f64).sqrt();
    let rms_error = (sum_sq_error / (end - start) as f64).sqrt();
    let error_ratio = rms_error / rms_signal;
    assert!(
        error_ratio < 1e-3,
        "mix=0 error ratio {error_ratio:.6} should be < 1e-3"
    );
}

#[test]
fn multiple_peaks_shift_independently() {
    // Two peaks at different frequencies should each shift to their
    // own target bin without interfering.
    let window_size = 128;
    let hop_size = 32;
    let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
    shifter.set_shift_semitones(12.0); // octave up, ratio = 2.0
    shifter.set_mono(true);

    let mut spectrum = vec![0.0f32; window_size];
    // Peak at bin 8.
    spectrum[16] = 1.0;
    // Peak at bin 20.
    spectrum[40] = 0.8;

    shifter.transform(&mut spectrum);

    // Bin 8 → target 16, bin 20 → target 40.
    let mag_16 = spectrum[32].hypot(spectrum[33]);
    let mag_40 = spectrum[2 * 40].hypot(spectrum[2 * 40 + 1]);

    assert!(
        mag_16 > 0.5,
        "target of bin 8 should have energy: {mag_16}"
    );
    assert!(
        mag_40 > 0.4,
        "target of bin 20 should have energy: {mag_40}"
    );

    // Original positions should be mostly empty.
    let mag_8 = spectrum[16].hypot(spectrum[17]);
    let mag_20 = spectrum[40].hypot(spectrum[41]);
    // bin 8 in the output IS the target of the first peak (16→32,
    // but bin 8 = spectrum[16..17] which is now target of bin 8).
    // Actually bin 8 in output = spectrum[16] which is target bin 16's
    // data.  Let me check the original bins:
    // Original bin 8 is at spectrum[16].  Target for peak at bin 8 is
    // bin 16 (spectrum[32]).  So spectrum[16] should be near zero
    // (no peak targets it).  But wait — bin 16 in the output
    // IS the target of peak 8.  Hmm, let me re-examine.
    // Target 16 is at spectrum index 2*16 = 32.  So spectrum[16] is
    // bin 8 in output.  Nothing targets bin 8, so it should be zero.
    assert!(
        mag_8 < 0.1,
        "original bin 8 position should be empty: {mag_8}"
    );
    assert!(
        mag_20 < 0.1,
        "original bin 20 position should be empty: {mag_20}"
    );
}
