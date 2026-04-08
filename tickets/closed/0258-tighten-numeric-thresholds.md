---
id: "0258"
title: Tighten patches-dsp numeric test thresholds
priority: low
created: 2026-04-02
---

## Summary

Several numeric thresholds in patches-dsp tests have excessive headroom,
meaning a significant regression could pass undetected. The test report measured
actual values and found gaps of 2-5 orders of magnitude between the threshold
and the measured result in several cases.

## Acceptance criteria

Tighten the following thresholds based on measured values (leave ~2x headroom
above the measured result, not 100x):

- [ ] `t6_snr_butterworth_lp_vs_f64_reference`: 60 dB -> 100 dB (measured 122.7)
- [ ] `t6_snr_svf_lp_vs_f64_reference`: 60 dB -> 120 dB (measured 141.7)
- [ ] `multi_partition_matches_naive`: 0.05 -> 1e-4 (measured 1e-6)
- [ ] `nu_long_ir_matches_naive`: 0.1 -> 0.01 (measured 3.2e-4)
- [ ] `identity_convolution`: 1e-3 -> 1e-5 (should be near machine epsilon)
- [ ] FFT `round_trip_identity`: 1e-5 -> 1e-6 (measured 3.58e-7)
- [ ] FFT `round_trip_large`: 1e-4 -> 1e-5 (measured 4.77e-7)
- [ ] `sinc_resample::identity_resample`: 0.01 -> 1e-4 (measured ~0)
- [ ] All existing tests still pass after tightening.
- [ ] Add a comment on each tightened threshold noting the measured value and
      date, so future reviewers know the basis.

## Notes

The goal is to make thresholds tight enough to catch regressions but loose
enough to tolerate platform variation (x86 vs ARM, different compiler
optimisation levels). The measured values are from a debug build on macOS
aarch64; release builds may differ slightly due to FMA fusion.
