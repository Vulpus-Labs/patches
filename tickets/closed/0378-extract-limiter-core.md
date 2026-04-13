---
id: "0378"
title: Extract shared LimiterCore to patches-dsp
priority: medium
created: 2026-04-13
---

## Summary

`Limiter` and `StereoLimiter` in `patches-modules` contain near-identical gain
envelope logic (target gain calculation, coefficient selection, smoothing) and
duplicated `ms_to_samples` / `compute_time_coeff` helper functions. The same
time-coefficient formula also appears in `EnvelopeFollower`.

## Acceptance criteria

- [ ] New `LimiterCore` struct in `patches-dsp` encapsulating the gain envelope state machine (peak tracking, gain smoothing, attack/release coefficient selection)
- [ ] `Limiter` and `StereoLimiter` delegate to `LimiterCore` (mono uses one instance, stereo uses one shared instance for linked gain reduction)
- [ ] Duplicated `ms_to_samples` and `compute_time_coeff` removed from both modules (use shared versions from 0381)
- [ ] All existing limiter tests pass unchanged
- [ ] No new clippy warnings

## Notes

Depends on 0381 (time utilities extraction). `LimiterCore` should own the peak
window, smoothed gain, and time coefficients. The module wrappers handle port
I/O and parameter mapping only.
