use super::*;

// ── T2 — Frequency response ──────────────────────────────────────────────

/// Drive a sinusoid at the cutoff frequency and measure steady-state
/// amplitude; compare against the theoretical transfer function magnitude.
///
/// The Chamberlin SVF bandpass peak is at the design frequency and has
/// magnitude ≈ 1 / q_damp at resonance.  For lowpass and highpass the
/// amplitude passes through a predictable level.  We allow ±1 dB error to
/// account for the bilinear/numerical approximation.
///
/// T2-LP: Lowpass at 100 Hz drive (well below 1 kHz cutoff) should pass ≈ 1.0.
#[test]
fn t2_frequency_response_lowpass_passband() {
    let fc = 1_000.0_f32;
    let q_norm = 0.0_f32; // flat/Butterworth
    let mut kernel = make_kernel(fc, q_norm);

    let drive_hz = 100.0;
    let amp = measure_steady_state_amplitude(&mut kernel, drive_hz, |(lp, _, _)| lp);

    // Passband: amplitude should be within ±1 dB of 1.0
    let db_err = db(amp).abs();
    assert!(
        db_err < 1.0,
        "LP passband at {drive_hz} Hz: amplitude={amp:.4}, dB_from_unity={db_err:.3}"
    );
}

/// T2-HP: Highpass at 10 kHz drive (well above 1 kHz cutoff) should be in the
/// passband (within ±3 dB of unity).
///
/// The Chamberlin SVF topology has a frequency-warping approximation that causes
/// a slight amplitude overshoot at high frequencies, so we allow ±3 dB here.
#[test]
fn t2_frequency_response_highpass_passband() {
    let fc = 1_000.0_f32;
    let q_norm = 0.0_f32;
    let mut kernel = make_kernel(fc, q_norm);

    let drive_hz = 10_000.0;
    let amp = measure_steady_state_amplitude(&mut kernel, drive_hz, |(_, hp, _)| hp);

    // Must be in passband: amplitude between -3 dB and +3 dB of unity
    assert!(
        amp > 0.7 && amp < 1.5,
        "HP passband at {drive_hz} Hz: amplitude={amp:.4}, expected in [0.7, 1.5]"
    );
}

/// T2-BP: Bandpass at cutoff frequency should peak near 1/q_damp.
#[test]
fn t2_frequency_response_bandpass_peak() {
    let fc = 1_000.0_f32;
    let q_norm = 0.5_f32;
    let f = svf_f(fc, SAMPLE_RATE);
    let d = q_to_damp(q_norm);
    let mut kernel = SvfKernel::new_static(f, d);

    // Drive at exact cutoff
    let amp = measure_steady_state_amplitude(&mut kernel, fc, |(_, _, bp)| bp);
    let theoretical = 1.0 / d; // peak gain at resonance
    let ratio = amp / theoretical;
    let db_err = db(ratio).abs();
    assert!(
        db_err < 1.0,
        "BP peak at fc={fc} Hz: amplitude={amp:.4}, theoretical={theoretical:.4}, dB_err={db_err:.3}"
    );
}

// ── FFT-based frequency response tests ─────────────────────────────────

#[test]
fn lowpass_frequency_response_full() {
    use crate::test_support::{assert_passband_flat, assert_stopband_below, magnitude_response_db};

    let mut kernel = make_kernel(1_000.0, 0.0);
    let fft_size = 1024;

    // Collect lowpass impulse response
    let mut ir = Vec::with_capacity(fft_size);
    for i in 0..fft_size {
        let x = if i == 0 { 1.0_f32 } else { 0.0 };
        let (lp, _, _) = kernel.tick(x);
        ir.push(lp);
    }

    let response_db = magnitude_response_db(&ir, fft_size);

    // bin_freq = bin_index * sample_rate / fft_size
    // bin_freq = bin * 48000 / 1024 ≈ bin * 46.875
    // 500 Hz → bin 10.67 → use bins 1..=10
    // 4000 Hz → bin 85.3 → use bins 86..=512
    let passband_end = (500.0 * fft_size as f32 / SAMPLE_RATE).floor() as usize; // 10
    let stopband_start = (4_000.0 * fft_size as f32 / SAMPLE_RATE).ceil() as usize; // 86
    let nyquist_bin = fft_size / 2;

    assert_passband_flat!(response_db, 1..=passband_end, 2.0);
    assert_stopband_below!(response_db, stopband_start..=nyquist_bin, -12.0);
}

#[test]
fn highpass_frequency_response_full() {
    use crate::test_support::{assert_passband_flat, assert_stopband_below, magnitude_response_db};

    let mut kernel = make_kernel(1_000.0, 0.0);
    let fft_size = 1024;

    // Collect highpass impulse response
    let mut ir = Vec::with_capacity(fft_size);
    for i in 0..fft_size {
        let x = if i == 0 { 1.0_f32 } else { 0.0 };
        let (_, hp, _) = kernel.tick(x);
        ir.push(hp);
    }

    let response_db = magnitude_response_db(&ir, fft_size);

    // 200 Hz → bin 4.27 → stopband bins 1..=4
    // 4000 Hz → bin 85.3, 20000 Hz → bin 426.7
    // Use bins 86..=426 for passband
    let stopband_end = (200.0 * fft_size as f32 / SAMPLE_RATE).floor() as usize; // 4
    let passband_start = (4_000.0 * fft_size as f32 / SAMPLE_RATE).ceil() as usize; // 86
    let passband_end = (20_000.0 * fft_size as f32 / SAMPLE_RATE).floor() as usize; // 426

    assert_stopband_below!(response_db, 1..=stopband_end, -12.0);
    assert_passband_flat!(response_db, passband_start..=passband_end, 3.0);
}

#[test]
fn bandpass_frequency_response_full() {
    use crate::test_support::{assert_peak_at_bin, magnitude_response_db};

    let mut kernel = make_kernel(1_000.0, 0.5);
    let fft_size = 1024;

    // Collect bandpass impulse response
    let mut ir = Vec::with_capacity(fft_size);
    for i in 0..fft_size {
        let x = if i == 0 { 1.0_f32 } else { 0.0 };
        let (_, _, bp) = kernel.tick(x);
        ir.push(bp);
    }

    let response_db = magnitude_response_db(&ir, fft_size);

    // 1000 Hz → bin 1000 * 1024 / 48000 ≈ 21.33
    let expected_bin = (1_000.0 * fft_size as f32 / SAMPLE_RATE).round() as usize; // 21
    assert_peak_at_bin!(response_db, expected_bin, 2);

    let peak_db = response_db[expected_bin];

    // Bins below 100 Hz: bin <= 2
    let low_end = (100.0 * fft_size as f32 / SAMPLE_RATE).floor() as usize; // 2
    for (bin, &v) in response_db.iter().enumerate().take(low_end + 1).skip(1) {
        assert!(
            v <= peak_db - 12.0,
            "bin {bin} at {v:.1} dB should be at least 12 dB below peak {peak_db:.1} dB"
        );
    }

    // Bins above 10 kHz: bin >= 214
    let high_start = (10_000.0 * fft_size as f32 / SAMPLE_RATE).ceil() as usize; // 214
    let nyquist_bin = fft_size / 2;
    for (bin, &v) in response_db.iter().enumerate().take(nyquist_bin + 1).skip(high_start) {
        assert!(
            v <= peak_db - 12.0,
            "bin {bin} at {v:.1} dB should be at least 12 dB below peak {peak_db:.1} dB"
        );
    }
}
