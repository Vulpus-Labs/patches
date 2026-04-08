//! SlotDeck integration tests.
//!
//! These tests exercise the full pipeline: OverlapBuffer ↔ ProcessorHandle,
//! including threaded round-trips, OLA/WOLA reconstruction, and FFT-based
//! spectral processing. Unit tests for individual pieces (config validation,
//! ActiveWindows, WindowBuffer) live alongside their implementations.

use patches_dsp::slot_deck::{FilledSlot, OverlapBuffer, SlotDeckConfig};
use patches_dsp::{RealPackedFft, SpectralPitchShifter, WindowBuffer};

#[test]
fn startup_silence() {
    // Before any full window has been filled, the overlap buffer outputs silence.
    let cfg = SlotDeckConfig::new(64, 2, 16).expect("valid config");
    let (mut buf, _handle) = OverlapBuffer::new_unthreaded(cfg);
    for _ in 0..32 {
        buf.write(1.0);
        assert_eq!(buf.read(), 0.0, "should be silent before pipeline fills");
    }
}

#[test]
fn round_trip_identity_inline() {
    // Processor simulated synchronously: identity transform (in-place passthrough).
    // After total_latency samples the output should reproduce the input.
    let cfg = SlotDeckConfig::new(64, 2, 16).expect("valid config");
    let latency = cfg.total_latency();
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);

    let mut outputs = Vec::new();
    for i in 0..(latency * 2) {
        let input = (i as f32) * 0.001;
        buf.write(input);

        // Inline processor: pop, pass through (no-op), push back.
        while let Some(slot) = handle.pop() {
            let _ = handle.push(slot);
        }

        outputs.push(buf.read());
    }

    // After latency samples the output should be non-zero.
    assert!(
        outputs[latency..].iter().any(|&x| x != 0.0),
        "output should be non-zero after pipeline fills"
    );
}

#[test]
fn round_trip_identity_threaded() {
    // Real spawned processing thread: identity transform.
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    let cfg = SlotDeckConfig::new(64, 2, 16).expect("valid config");
    let latency = cfg.total_latency();
    let window_size = cfg.window_size;
    let (mut buf, handle) = OverlapBuffer::new_unthreaded(cfg);

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_proc = shutdown.clone();

    let processor = thread::spawn(move || {
        let mut h = handle;
        // Identity: just push back unmodified.
        while !shutdown_proc.load(Ordering::Relaxed) {
            if let Some(mut slot) = h.pop() {
                // Spin on push since buffer must circulate.
                loop {
                    match h.push(slot) {
                        Ok(()) => break,
                        Err(s) => {
                            slot = s;
                            if shutdown_proc.load(Ordering::Relaxed) { return; }
                            std::hint::spin_loop();
                        }
                    }
                }
            } else {
                std::hint::spin_loop();
            }
        }
        // After shutdown: drain remaining.
        while let Some(slot) = h.pop() {
            let _ = h.push(slot);
        }
    });

    // Phase 1: feed input samples.
    let n_samples = latency * 4;
    for i in 0..n_samples {
        buf.write((i as f32) * 0.001);
        buf.read(); // advance read head
    }

    // Phase 2: stop the processor.
    shutdown.store(true, Ordering::Relaxed);
    processor.join().expect("processor thread panicked");

    // Phase 3: flush — additional write/read cycles drain any results
    // sitting in the inbound ring buffer.
    let mut saw_nonzero = false;
    for _ in 0..(window_size * 2) {
        buf.write(0.0);
        if buf.read() != 0.0 {
            saw_nonzero = true;
            break;
        }
    }

    assert!(saw_nonzero, "output should be non-zero after pipeline fills");
}

#[test]
fn late_frame_discarded() {
    // A result frame pushed after read_head has advanced past its window end
    // should be silently discarded (no panic, no output contribution).
    let cfg = SlotDeckConfig::new(64, 2, 16).expect("valid config");
    let latency = cfg.total_latency();
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);

    // Drain the pipeline without the processor doing any work.
    for i in 0..(latency * 2) {
        buf.write((i as f32) * 0.001);
        let _ = buf.read();
    }

    // Now inject a result with start=0 (long past) — should be discarded.
    let stale_data: Box<[f32]> = vec![1.0f32; 64].into_boxed_slice();
    let _ = handle.push(FilledSlot { start: 0, data: stale_data });

    // Output should still be 0 (or whatever the pipeline produces, but not
    // the 1.0 from the stale frame).
    let out = buf.read();
    assert!(out.abs() < 1.0, "stale frame should not contribute to output");
}

// ---------------------------------------------------------------------------
// OLA with Hann applied once (COLA property, no normalisation needed)
// ---------------------------------------------------------------------------

// Run plain OLA: constant-1 input, processor applies `window` once.
fn run_ola(cfg: SlotDeckConfig, window: &WindowBuffer) -> Vec<f32> {
    let n_samples = cfg.total_latency() * 4;
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);
    let mut outputs = Vec::with_capacity(n_samples);

    for _ in 0..n_samples {
        buf.write(1.0_f32);
        while let Some(mut slot) = handle.pop() {
            // Apply window in-place.
            let mut windowed = vec![0.0_f32; slot.data.len()].into_boxed_slice();
            window.apply_into(&slot.data, &mut windowed);
            slot.data.copy_from_slice(&windowed);
            let _ = handle.push(slot);
        }
        outputs.push(buf.read());
    }

    outputs
}

#[test]
fn ola_hann_overlap_2() {
    // Hann window w = sin², F=2 (50% overlap).
    // COLA identity: sin²(θ) + sin²(θ+π/2) = sin²(θ) + cos²(θ) = 1.
    // Overlap-add sum = 1 in steady state → no normalisation required.
    let cfg = SlotDeckConfig::new(32, 2, 16).unwrap();
    let latency = cfg.total_latency();
    let window = WindowBuffer::new(cfg.window_size, |n| (std::f32::consts::PI * n).sin().powi(2));
    let outputs = run_ola(cfg, &window);

    let check_from = latency + 32;
    for (i, &s) in outputs[check_from..].iter().enumerate() {
        assert!(
            (s - 1.0).abs() < 1e-5,
            "OLA Hann F=2 failed at sample {}: expected 1.0, got {}",
            check_from + i,
            s
        );
    }
}

#[test]
fn ola_hann_overlap_4() {
    // Hann window w = sin², F=4 (75% overlap).
    // Overlap-add sum = 2 in steady state (Hann COLA sum for 75% overlap).
    // Without normalisation the raw output is 2.0, not 1.0 — demonstrating
    // why WOLA is needed when an additional synthesis window is also applied.
    let cfg = SlotDeckConfig::new(32, 4, 16).unwrap();
    let latency = cfg.total_latency();
    let window = WindowBuffer::new(cfg.window_size, |n| (std::f32::consts::PI * n).sin().powi(2));
    let outputs = run_ola(cfg, &window);

    let check_from = latency + 32;
    for (i, &s) in outputs[check_from..].iter().enumerate() {
        assert!(
            (s - 2.0).abs() < 1e-5,
            "OLA Hann F=4 failed at sample {}: expected 2.0, got {}",
            check_from + i,
            s
        );
    }
}

// ---------------------------------------------------------------------------
// WOLA with Hann applied twice (analysis + synthesis window, normalisation required)
// ---------------------------------------------------------------------------

// Run WOLA: constant-1 input, processor applies `window` twice (analysis before
// processing, synthesis after — simulating a spectral processor that preserves signal).
// The effective OLA window is w², which is not COLA, so normalisation is needed.
fn run_wola(cfg: SlotDeckConfig, window: &WindowBuffer) -> Vec<f32> {
    let synth_window = window.normalised_wola(cfg.hop_size());
    let n_samples = cfg.total_latency() * 4;
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);
    let mut outputs = Vec::with_capacity(n_samples);

    for _ in 0..n_samples {
        buf.write(1.0_f32);
        while let Some(mut slot) = handle.pop() {
            // Analysis window: applied to input before spectral processing.
            let mut analysis = vec![0.0_f32; slot.data.len()].into_boxed_slice();
            window.apply_into(&slot.data, &mut analysis);
            // (spectral processing would go here — identity for this test)
            // Pre-normalised synthesis window — WOLA correction baked in.
            slot.data.fill(0.0);
            synth_window.apply_into(&analysis, &mut slot.data);
            let _ = handle.push(slot);
        }
        outputs.push(buf.read());
    }

    outputs
}

#[test]
fn wola_hann_overlap_2() {
    // Hann applied twice → effective window w² = sin⁴, which is not COLA.
    // normalised_wola computes norm[p] = 1 / Σ_k w[p+k·hop]², compensating exactly.
    let cfg = SlotDeckConfig::new(32, 2, 16).unwrap();
    let latency = cfg.total_latency();
    let window = WindowBuffer::new(cfg.window_size, |n| (std::f32::consts::PI * n).sin().powi(2));
    let outputs = run_wola(cfg, &window);

    let check_from = latency + 32;
    for (i, &s) in outputs[check_from..].iter().enumerate() {
        assert!(
            (s - 1.0).abs() < 1e-5,
            "WOLA Hann F=2 failed at sample {}: expected 1.0, got {}",
            check_from + i,
            s
        );
    }
}

#[test]
fn wola_hann_overlap_4() {
    // Same as above with 75% overlap. normalised_wola handles the different
    // hop-phase sums automatically.
    let cfg = SlotDeckConfig::new(32, 4, 16).unwrap();
    let latency = cfg.total_latency();
    let window = WindowBuffer::new(cfg.window_size, |n| (std::f32::consts::PI * n).sin().powi(2));
    let outputs = run_wola(cfg, &window);

    let check_from = latency + 32;
    for (i, &s) in outputs[check_from..].iter().enumerate() {
        assert!(
            (s - 1.0).abs() < 1e-5,
            "WOLA Hann F=4 failed at sample {}: expected 1.0, got {}",
            check_from + i,
            s
        );
    }
}

// ---------------------------------------------------------------------------
// Overload and starvation
// ---------------------------------------------------------------------------

#[test]
fn write_overload_does_not_block() {
    // If the outbound ring buffer is full the write must silently drop, not block.
    let cfg = SlotDeckConfig::new(16, 2, 8).expect("valid config");
    let pool_size = cfg.pool_size();
    let window_size = cfg.window_size;
    let (mut buf, _handle) = OverlapBuffer::new_unthreaded(cfg);

    // Write many more samples than the pool can hold — must not block or panic.
    for i in 0..(pool_size * window_size * 4) {
        buf.write((i as f32) * 0.001);
    }
}

#[test]
fn pool_starvation_degrades_gracefully() {
    // If no free buffers are available, writes are dropped silently.
    let cfg = SlotDeckConfig::new(32, 2, 8).expect("valid config");
    let pool_size = cfg.pool_size();
    let window_size = cfg.window_size;
    let (mut buf, _handle_dropped) = OverlapBuffer::new_unthreaded(cfg);

    // Write enough samples to exhaust the pool and trigger starvation.
    for i in 0..(pool_size * window_size * 2) {
        buf.write((i as f32) * 0.001); // must not panic
    }
}

// ---------------------------------------------------------------------------
// Pool exhaustion recovery and edge cases
// ---------------------------------------------------------------------------

#[test]
fn pool_starvation_recovery() {
    // After all buffers are consumed (no recycling), returning buffers should
    // restore normal operation — no stuck state.
    let cfg = SlotDeckConfig::new(32, 2, 8).expect("valid config");
    let latency = cfg.total_latency();
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);

    // Phase 1: Write samples but don't process — starve the pool.
    let starvation_samples = latency * 2;
    for i in 0..starvation_samples {
        buf.write((i as f32) * 0.001);
        let _ = buf.read(); // output will be silence
    }

    // Phase 2: Now start processing — pop all available, push back.
    let mut recovered_nonzero = false;
    let recovery_samples = latency * 4;
    for i in 0..recovery_samples {
        buf.write(1.0);

        // Process all available frames (identity)
        while let Some(slot) = handle.pop() {
            let _ = handle.push(slot);
        }

        let v = buf.read();
        if v.abs() > 0.0 && i > latency {
            recovered_nonzero = true;
        }
    }

    assert!(
        recovered_nonzero,
        "output should become non-zero after recovery from pool starvation"
    );
}

#[test]
fn slow_processor_degrades_gracefully() {
    // A processor that only processes every Nth frame should produce silence
    // for missed frames, not corruption.
    let cfg = SlotDeckConfig::new(32, 2, 8).expect("valid config");
    let latency = cfg.total_latency();
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);

    let mut frame_count = 0usize;
    let n_samples = latency * 4;
    let mut all_finite = true;

    for _ in 0..n_samples {
        buf.write(1.0);

        // Only process every 3rd frame — simulate slow processor
        while let Some(slot) = handle.pop() {
            frame_count += 1;
            if frame_count % 3 == 0 {
                let _ = handle.push(slot);
            }
            // Frames not pushed back are dropped — the buffer is lost.
            // This tests that the audio thread degrades gracefully.
        }

        let v = buf.read();
        if !v.is_finite() {
            all_finite = false;
        }
    }

    assert!(all_finite, "all output samples should be finite even with slow processor");
}

#[test]
fn return_channel_full_does_not_panic() {
    // If the inbound ring buffer is full, push should return Err, not panic.
    let cfg = SlotDeckConfig::new(32, 2, 8).expect("valid config");
    let pool_size = cfg.pool_size();
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);

    // Fill the inbound channel by pushing many result frames without reading.
    for _ in 0..pool_size * 2 {
        buf.write(1.0);

        while let Some(slot) = handle.pop() {
            // Push result — may fail if channel full; that's OK.
            let _ = handle.push(slot);
        }
        // Deliberately not calling buf.read() to let results accumulate.
    }

    // Now read should drain without panic.
    for _ in 0..pool_size * 4 {
        buf.write(0.0);
        let _ = buf.read(); // must not panic
    }
}

#[test]
fn buffer_recycling_preserves_pool() {
    // After a burst of fill/process/drain cycles, the pool should recover
    // its buffers and continue producing filled slots.
    let cfg = SlotDeckConfig::new(32, 2, 8).expect("valid config");
    let latency = cfg.total_latency();
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);

    // Phase 1: run a full cycle — write, process, read.
    let n_samples = latency * 3;
    for i in 0..n_samples {
        buf.write((i as f32) * 0.001);
        while let Some(slot) = handle.pop() {
            let _ = handle.push(slot);
        }
        let _ = buf.read();
    }

    // Phase 2: continue — should still produce non-zero output.
    let mut saw_nonzero = false;
    for i in 0..n_samples {
        buf.write(1.0);
        while let Some(slot) = handle.pop() {
            let _ = handle.push(slot);
        }
        let v = buf.read();
        if v != 0.0 && i > latency {
            saw_nonzero = true;
        }
    }

    assert!(
        saw_nonzero,
        "after recycling, pipeline should continue producing non-zero output"
    );
}

// ---------------------------------------------------------------------------
// FFT-based brick-wall low-pass filter via WOLA
// ---------------------------------------------------------------------------

/// Run a crude FFT brick-wall low-pass through the WOLA pipeline.
fn run_fft_lowpass(input: &[f32], window_size: usize, overlap_factor: usize) -> Vec<f32> {
    let cfg = SlotDeckConfig::new(window_size, overlap_factor, window_size).unwrap();
    let analysis_window = WindowBuffer::new(window_size, |n| {
        (std::f32::consts::PI * n).sin().powi(2)
    });
    let synth_window = analysis_window.normalised_wola(cfg.hop_size());
    let fft = RealPackedFft::new(window_size);

    let n_samples = input.len() + cfg.total_latency();
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);
    let mut outputs = Vec::with_capacity(n_samples);

    for i in 0..n_samples {
        let sample = if i < input.len() { input[i] } else { 0.0 };
        buf.write(sample);

        while let Some(mut slot) = handle.pop() {
            // Analysis window
            let mut frame = vec![0.0f32; slot.data.len()].into_boxed_slice();
            analysis_window.apply_into(&slot.data, &mut frame);

            // Forward FFT
            fft.forward(&mut frame);

            // Zero all bins above half-Nyquist (i.e. above bin N/4).
            let quarter_n = window_size / 4;
            frame[1] = 0.0;
            for k in (quarter_n + 1)..(window_size / 2) {
                frame[2 * k] = 0.0;
                frame[2 * k + 1] = 0.0;
            }

            // Inverse FFT
            fft.inverse(&mut frame);

            // Pre-normalised synthesis window — WOLA correction baked in.
            slot.data.fill(0.0);
            synth_window.apply_into(&frame, &mut slot.data);
            let _ = handle.push(slot);
        }

        outputs.push(buf.read());
    }

    outputs
}

/// Measure RMS power of the steady-state region of an output signal.
fn rms(signal: &[f32]) -> f32 {
    let sum_sq: f32 = signal.iter().map(|&x| x * x).sum();
    (sum_sq / signal.len() as f32).sqrt()
}

#[test]
fn fft_lowpass_passes_low_frequency() {
    let window_size = 256;
    let n_input = window_size * 16;
    let freq_bin = 3.0;
    let input: Vec<f32> = (0..n_input)
        .map(|i| (2.0 * std::f32::consts::PI * freq_bin * i as f32 / window_size as f32).sin())
        .collect();

    let output = run_fft_lowpass(&input, window_size, 4);

    let skip = window_size * 3;
    let steady = &output[skip..skip + window_size * 8];
    let input_rms = rms(&input[skip..skip + window_size * 8]);
    let output_rms = rms(steady);

    assert!(
        output_rms > input_rms * 0.9,
        "low-freq signal should pass: input_rms={input_rms}, output_rms={output_rms}"
    );
}

#[test]
fn fft_lowpass_attenuates_high_frequency() {
    let window_size = 256;
    let n_input = window_size * 16;
    let freq_bin = 96.0;
    let input: Vec<f32> = (0..n_input)
        .map(|i| (2.0 * std::f32::consts::PI * freq_bin * i as f32 / window_size as f32).sin())
        .collect();

    let output = run_fft_lowpass(&input, window_size, 4);

    let skip = window_size * 3;
    let steady = &output[skip..skip + window_size * 8];
    let input_rms = rms(&input[skip..skip + window_size * 8]);
    let output_rms = rms(steady);

    assert!(
        output_rms < input_rms * 0.1,
        "high-freq signal should be attenuated: input_rms={input_rms}, output_rms={output_rms}"
    );
}

#[test]
fn fft_lowpass_mixed_signal_preserves_low_removes_high() {
    let window_size = 256;
    let n_input = window_size * 16;
    let lo_bin = 4.0;
    let hi_bin = 100.0;
    let input: Vec<f32> = (0..n_input)
        .map(|i| {
            let t = i as f32 / window_size as f32;
            let lo = (2.0 * std::f32::consts::PI * lo_bin * t).sin();
            let hi = (2.0 * std::f32::consts::PI * hi_bin * t).sin();
            lo + hi
        })
        .collect();

    let output = run_fft_lowpass(&input, window_size, 4);

    let skip = window_size * 3;
    let len = window_size * 8;
    let steady = &output[skip..skip + len];

    let expected_lo: Vec<f32> = (skip..skip + len)
        .map(|i| (2.0 * std::f32::consts::PI * lo_bin * i as f32 / window_size as f32).sin())
        .collect();

    let error_rms = rms(
        &steady
            .iter()
            .zip(expected_lo.iter())
            .map(|(&a, &b)| a - b)
            .collect::<Vec<_>>(),
    );
    let signal_rms = rms(&expected_lo);

    assert!(
        error_rms < signal_rms * 0.2,
        "filtered output should match low component: signal_rms={signal_rms}, error_rms={error_rms}"
    );
}

// ---------------------------------------------------------------------------
// Spectral pitch shifter via WOLA
// ---------------------------------------------------------------------------

fn run_pitch_shift(
    input: &[f32],
    window_size: usize,
    overlap_factor: usize,
    semitones: f32,
) -> Vec<f32> {
    let cfg = SlotDeckConfig::new(window_size, overlap_factor, window_size).unwrap();
    let hop_size = cfg.hop_size();
    let analysis_window = WindowBuffer::new(window_size, |n| {
        (std::f32::consts::PI * n).sin().powi(2)
    });
    let synth_window = analysis_window.normalised_wola(hop_size);
    let fft = RealPackedFft::new(window_size);
    let mut shifter = SpectralPitchShifter::new(window_size, hop_size);
    shifter.set_shift_semitones(semitones);

    let n_samples = input.len() + cfg.total_latency();
    let (mut buf, mut handle) = OverlapBuffer::new_unthreaded(cfg);
    let mut outputs = Vec::with_capacity(n_samples);

    for i in 0..n_samples {
        let sample = if i < input.len() { input[i] } else { 0.0 };
        buf.write(sample);

        while let Some(mut slot) = handle.pop() {
            // Analysis window
            let mut frame = vec![0.0f32; slot.data.len()].into_boxed_slice();
            analysis_window.apply_into(&slot.data, &mut frame);

            // Forward FFT → pitch shift → inverse FFT
            fft.forward(&mut frame);
            shifter.transform(&mut frame);
            fft.inverse(&mut frame);

            // Pre-normalised synthesis window — WOLA correction baked in.
            slot.data.fill(0.0);
            synth_window.apply_into(&frame, &mut slot.data);
            let _ = handle.push(slot);
        }

        outputs.push(buf.read());
    }

    outputs
}

fn dominant_bin(signal: &[f32], fft: &RealPackedFft) -> usize {
    let n = fft.len();
    let mut buf = vec![0.0f32; n];
    buf[..signal.len().min(n)].copy_from_slice(&signal[..signal.len().min(n)]);
    fft.forward(&mut buf);

    let mut best_k = 0;
    let mut best_mag = 0.0f32;
    for k in 1..(n / 2) {
        let mag = buf[2 * k].hypot(buf[2 * k + 1]);
        if mag > best_mag {
            best_mag = mag;
            best_k = k;
        }
    }
    best_k
}

#[test]
fn pitch_shift_octave_up_doubles_frequency() {
    let window_size = 1024;
    let source_bin: usize = 8;
    let n_input = window_size * 32;
    let input: Vec<f32> = (0..n_input)
        .map(|i| {
            (2.0 * std::f32::consts::PI * source_bin as f32 * i as f32 / window_size as f32).sin()
        })
        .collect();

    let output = run_pitch_shift(&input, window_size, 4, 12.0);

    let skip = window_size * 6;
    let chunk = &output[skip..skip + window_size];
    let fft = RealPackedFft::new(window_size);
    let bin = dominant_bin(chunk, &fft);

    assert!(
        (bin as i32 - (source_bin * 2) as i32).unsigned_abs() <= 1,
        "expected dominant bin near {}, got {bin}",
        source_bin * 2
    );
}

#[test]
fn pitch_shift_octave_down_halves_frequency() {
    let window_size = 1024;
    let source_bin: usize = 16;
    let n_input = window_size * 32;
    let input: Vec<f32> = (0..n_input)
        .map(|i| {
            (2.0 * std::f32::consts::PI * source_bin as f32 * i as f32 / window_size as f32).sin()
        })
        .collect();

    let output = run_pitch_shift(&input, window_size, 4, -12.0);

    let skip = window_size * 6;
    let chunk = &output[skip..skip + window_size];
    let fft = RealPackedFft::new(window_size);
    let bin = dominant_bin(chunk, &fft);

    assert!(
        (bin as i32 - (source_bin / 2) as i32).unsigned_abs() <= 1,
        "expected dominant bin near {}, got {bin}",
        source_bin / 2
    );
}

#[test]
fn pitch_shift_identity_preserves_signal() {
    let window_size = 256;
    let source_bin = 5;
    let n_input = window_size * 16;
    let input: Vec<f32> = (0..n_input)
        .map(|i| {
            (2.0 * std::f32::consts::PI * source_bin as f32 * i as f32 / window_size as f32).sin()
        })
        .collect();

    let output = run_pitch_shift(&input, window_size, 4, 0.0);

    let skip = window_size * 4;
    let len = window_size * 8;
    let steady_out = &output[skip..skip + len];
    let steady_in = &input[skip..skip + len];

    let error: Vec<f32> = steady_out
        .iter()
        .zip(steady_in.iter())
        .map(|(&a, &b)| a - b)
        .collect();
    let error_rms = rms(&error);
    let signal_rms = rms(steady_in);

    assert!(
        error_rms < signal_rms * 0.15,
        "identity shift should preserve signal: signal_rms={signal_rms}, error_rms={error_rms}"
    );
}

#[test]
fn pitch_shift_fifth_up() {
    let window_size = 1024;
    let source_bin = 10;
    let expected_bin = (source_bin as f32 * 2.0f32.powf(7.0 / 12.0)).round() as usize;
    let n_input = window_size * 32;
    let input: Vec<f32> = (0..n_input)
        .map(|i| {
            (2.0 * std::f32::consts::PI * source_bin as f32 * i as f32 / window_size as f32).sin()
        })
        .collect();

    let output = run_pitch_shift(&input, window_size, 4, 7.0);

    let skip = window_size * 6;
    let chunk = &output[skip..skip + window_size];
    let fft = RealPackedFft::new(window_size);
    let bin = dominant_bin(chunk, &fft);

    assert!(
        (bin as i32 - expected_bin as i32).unsigned_abs() <= 1,
        "expected dominant bin near {expected_bin}, got {bin}"
    );
}
