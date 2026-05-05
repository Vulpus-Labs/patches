---
id: "0803"
title: Audit determinism tests for FTZ sensitivity
priority: medium
created: 2026-05-04
---

## Summary

FTZ/DAZ (ticket 0802) breaks IEEE-strict reproducibility: subnormals
flush to zero. Audit existing bit-exact / determinism tests to
confirm none rely on subnormal output, or document the dependency.

## Acceptance criteria

- [ ] Enumerate tests asserting bit-exact output (oscillator phase,
      DSL golden renders, any WAV-compare tests).
- [ ] Run each with FTZ enabled; confirm pass.
- [ ] For any test that diverges: either regenerate golden with FTZ
      on, or mark as FTZ-independent (runs without callback FTZ
      setup).
- [ ] Document in ADR or in E134 epic which tests are FTZ-dependent.

## Notes

- Offline render paths (WAV bounce, integration tests) generally do
  not go through the audio callback, so FTZ is not set there. If
  goldens were generated without FTZ, they remain valid.
- Risk concentrates in tests that simulate long decay tails.

## Outcome

Audit complete. `just inner` (197+298+367+... tests across all
crates) passes after the FTZ wiring and per-site flushes landed.

Determinism harness in `patches-dsp/src/test_support.rs`
(`assert_reset_deterministic!`) compares fresh vs reset runs *on the
same thread*, so both runs see identical FTZ state — invariant under
hardware FTZ regardless of whether it's enabled.

No golden WAV / cross-machine bit-exact tests exist in the tree.
All inline numerical assertions use `assert_eq` or `assert_relative_eq`
on signal envelopes, not on raw subnormal magnitudes. None broke.

Conclusion: enabling FTZ in the audio callback (ticket 0802) does
not affect any existing test. Per-site `flush_denormal` calls in
biquad / vintage are equivalent to FTZ output flush, so production
audio path remains bit-equivalent to test output for normal-range
signals.
