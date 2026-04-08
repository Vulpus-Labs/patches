---
id: "0236"
title: SVF frequency response via impulse-response FFT
priority: medium
created: 2026-04-01
---

## Summary

The SVF frequency response tests (T2-LP, T2-HP, T2-BP, T3-DC, T3-Nyquist)
each drive the filter with a sinusoid at a single frequency, requiring
4096 warmup + 1024 measurement samples per test. An impulse-response FFT gives
the full transfer function in one pass and enables multi-frequency assertions
in a single test.

Add new FFT-based frequency response tests alongside the existing ones.

Depends on T-0234.

## Acceptance criteria

- [ ] New test `lowpass_frequency_response_full` in `svf.rs`:
      - Construct an `SvfKernel` at 1 kHz cutoff, q_norm = 0.0 (Butterworth),
        sample rate 48 kHz.
      - Feed a unit impulse through the lowpass output, collect ≥ 1024 samples.
      - Compute `magnitude_response_db` via the T-0234 helper.
      - Assert passband (bins corresponding to 0–500 Hz) is flat within ±1 dB
        using `assert_passband_flat!`.
      - Assert stopband (bins corresponding to 4 kHz–Nyquist) is below -12 dB
        using `assert_stopband_below!`.

- [ ] New test `highpass_frequency_response_full` in `svf.rs`:
      - Same kernel, highpass output.
      - Assert stopband (0–200 Hz) is below -12 dB.
      - Assert passband (4 kHz–20 kHz) is within ±3 dB (accounting for
        Chamberlin frequency warping at high frequencies).

- [ ] New test `bandpass_frequency_response_full` in `svf.rs`:
      - Kernel at 1 kHz cutoff, q_norm = 0.5.
      - Assert peak is at the cutoff bin using `assert_peak_at_bin!`.
      - Assert bins below 100 Hz and above 10 kHz are at least 12 dB below
        the peak.

- [ ] The existing sinusoid-driven tests (T2-LP, T2-HP, T2-BP, T3-*) are
      kept — they serve as independent cross-checks and test steady-state
      convergence behaviour that impulse-response tests do not cover.

- [ ] `cargo test -p patches-dsp` passes.
- [ ] `cargo clippy -- -D warnings` clean.

## Notes

The Chamberlin SVF has known frequency warping near Nyquist. The FFT-based
tests will make this visible as a gentle passband rise in the highpass
response. The ±3 dB tolerance for HP passband accommodates this.

Impulse response length matters: a 1024-sample IR at 48 kHz gives ~47 Hz bin
resolution (48000/1024). For a 1 kHz cutoff this is adequate. If finer
resolution is needed, use a 2048 or 4096-point FFT with zero-padding.
