---
id: "0239"
title: THD helper and approximation distortion tests
priority: low
created: 2026-04-01
---

## Summary

The `fast_sine`, `fast_tanh`, and `lookup_sine` approximations in
`approximate.rs` are tested with max absolute error and RMS error against a
reference. For audio applications, total harmonic distortion (THD) is a more
meaningful metric: it measures how much energy the approximation adds at
harmonic frequencies relative to the fundamental. An FFT-based THD helper
enables expressing accuracy as "THD < -60 dB" rather than "max error < 0.002".

Depends on T-0234.

## Acceptance criteria

- [ ] New helper `thd_db(signal: &[f32], fundamental_bin: usize, fft_size: usize) -> f32`
      in `test_support.rs`:
      - FFT the signal (must be `fft_size` samples, power of 2).
      - Compute the magnitude of the fundamental bin.
      - Compute the sum-of-squares of magnitudes at harmonic bins
        (2×, 3×, 4×, … up to N/2).
      - Return `10 * log10(harmonic_power / fundamental_power)` in dB.
      - Helper is `pub(crate)`, `#[cfg(test)]`.

- [ ] New test `fast_sine_thd` in `approximate.rs`:
      - Generate one or more complete periods of `fast_sine` output at a
        frequency that lands on an exact FFT bin (e.g. bin 8 of a 1024-point
        FFT = 8 cycles in 1024 samples).
      - Assert THD < -40 dB (the polynomial approximation is moderate quality;
        adjust threshold to match actual performance).

- [ ] New test `lookup_sine_thd` in `approximate.rs`:
      - Same structure. The lookup table should achieve better THD than the
        polynomial — assert THD < -50 dB (adjust to actual).

- [ ] New test `fast_tanh_thd` in `approximate.rs`:
      - Drive `fast_tanh` with a moderate-amplitude sinusoid (e.g. amplitude
        0.5 to stay in the quasi-linear region).
      - Assert THD < -40 dB.
      - Optionally: a second test at higher amplitude (e.g. 0.9) to document
        the expected distortion increase.

- [ ] The existing error-based tests (`test_fast_sine_accuracy`,
      `test_fast_sine_snr`, `test_fast_tanh_accuracy`, etc.) are kept — they
      test different properties (worst-case error, RMS error vs reference).

- [ ] `cargo test -p patches-dsp` passes.
- [ ] `cargo clippy -- -D warnings` clean.

## Notes

THD measurement requires the input signal to be at an exact FFT bin to avoid
spectral leakage. Use `signal[i] = fast_sine(i as f32 * bin / fft_size as f32)`
(or equivalent phase accumulation) to ensure exact bin alignment.

The THD thresholds in the acceptance criteria are initial guesses. During
implementation, measure the actual THD first and set the threshold ~3 dB above
(more lenient) to avoid brittle tests while still catching regressions.
