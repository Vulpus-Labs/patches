---
id: "0217"
title: Add T4 stability tests for HalfbandInterpolator and DelayBuffer under modulation
priority: low
created: 2026-03-30
---

## Summary

`HalfbandInterpolator` and `DelayBuffer` both lack tests for behaviour under
rapid parameter changes. Specifically: modulating delay time rapidly (using
`DelayBuffer` with a changing read offset) and changing the oversampling ratio
of `HalfbandInterpolator` should not cause unbounded growth or NaN.

## Acceptance criteria

- [ ] `delay_buffer.rs`: T4 test — process 10,000 samples with read offset
  modulated rapidly across the full delay range; verify output stays within
  `[-2.0, 2.0]` for a `[-1.0, 1.0]` input.
- [ ] `interpolator.rs`: T4 test — process 10,000 samples of max-amplitude
  input; verify output stays finite and bounded.
- [ ] Each test carries a `/// T4 — stability and convergence` doc comment.
- [ ] `cargo test -p patches-dsp` passes, `cargo clippy` clean.

## Notes

The delay modulation test is particularly important because `ThiranInterp` (used
for fractional delay) is an all-pass filter; rapid coefficient changes can
transiently exceed the input range.

ADR 0022 technique reference: **T4** — stability and convergence.
