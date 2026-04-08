---
id: "0257"
title: PolyBiquad spectral and stability coverage
priority: medium
created: 2026-04-02
---

## Summary

`PolyBiquad` has 4 tests that verify coefficient mechanics (static fan-out,
ramping, delta advancement, voice independence). The mono path (`MonoBiquad`) has
thorough spectral coverage: frequency response (T2), SNR vs f64 reference (T6),
high-Q stability (T4), and FFT-based passband/stopband checks (T0240). None of
these exist for the poly path. A bug affecting only the 16-voice SIMD-style
processing would go undetected.

## Acceptance criteria

- [ ] `poly_snr_matches_mono`: for each of 16 voices driven with the same input,
      the poly output matches the mono output within 1e-6 (or bit-identical).
      This is the poly equivalent of `poly_kernel_matches_mono_kernel` in SVF.
- [ ] `poly_stability_high_resonance`: all 16 voices remain bounded under high-Q
      noise input (equivalent of `t4_high_resonance_stability`).
- [ ] `poly_frequency_response_lowpass`: at least one voice checked with an
      FFT-based passband/stopband assertion matching the mono T0240 thresholds.
- [ ] All tests in `biquad.rs` unit test module.

## Notes

The mono-parity test is the highest-value addition — if poly matches mono
exactly, the existing mono spectral tests provide transitive coverage. The
stability and frequency response tests add defence in depth.
