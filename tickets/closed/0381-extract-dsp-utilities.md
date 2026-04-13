---
id: "0381"
title: Extract DcBlocker and time utilities to patches-dsp
priority: medium
created: 2026-04-13
---

## Summary

The Drive module contains a private `DcBlocker` struct (a reusable one-pole
highpass at ~5 Hz) and a private `quantize` function that duplicates
`BitcrusherKernel::quantize`. `ms_to_samples` and `compute_time_coeff` are
copy-pasted between `Limiter`, `StereoLimiter`, and `EnvelopeFollower`.

These are general-purpose DSP primitives that belong in `patches-dsp`.

## Acceptance criteria

- [ ] `DcBlocker` moved to `patches-dsp` as a public struct
- [ ] `ms_to_samples` and `compute_time_coeff` (or `time_coeff`) exposed as public functions in `patches-dsp`
- [ ] `EnvelopeFollower::time_coeff` delegates to or is replaced by the shared function
- [ ] Drive module uses `patches_dsp::DcBlocker` and `patches_dsp::BitcrusherKernel::quantize`
- [ ] Limiter and StereoLimiter use the shared time utility functions
- [ ] All existing tests pass
- [ ] No new clippy warnings

## Notes

Suggested locations: `patches-dsp/src/dc_blocker.rs` for `DcBlocker`,
`patches-dsp/src/time_utils.rs` for `ms_to_samples` / `compute_time_coeff`.
The `quantize` function doesn't need a new home — just make
`BitcrusherKernel::quantize` public and use it from Drive.
