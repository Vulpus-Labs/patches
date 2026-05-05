---
id: "0805"
title: Flush biquad TDFII linear mode and limiter_core gain envelope
priority: medium
created: 2026-05-04
---

## Summary

Two patches-dsp / patches-modules sites identified in the denormal
audit that lack mitigation:

1. `patches-dsp/src/biquad/mod.rs` `MonoBiquad::tick` and
   `PolyBiquad::tick_all` — TDFII state `s1`, `s2` unprotected when
   `saturate=false`. Saturating path has tanh scrubbing; linear path
   does not.
2. `limiter_core.rs` smoothed gain envelope: asymptotic
   `current_gain += coeff * (target - current_gain)` with no flush
   on long release.

## Acceptance criteria

- [ ] Biquad: flush_denormal on `s1`, `s2` writes when saturate is
      off (or unconditionally — measure cost).
- [ ] Limiter: flush_denormal on `current_gain` after smoothing
      step.
- [ ] No regression in existing biquad or limiter tests.
- [ ] `just inner -p patches-dsp` and touched module crate pass.

## Notes

- SVF/ladder paths already sanitize; not in scope.
- Biquad change must not perturb tanh-saturated output (only the
  linear path).
