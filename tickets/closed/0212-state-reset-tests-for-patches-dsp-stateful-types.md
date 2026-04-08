---
id: "0212"
title: Add T7 state-reset tests for patches-dsp stateful types
priority: medium
created: 2026-03-30
---

## Summary

Four stateful types in `patches-dsp` are missing T7 (determinism / state reset)
tests: `DelayBuffer`, `ThiranInterp`, `HalfbandInterpolator`, and `PeakWindow`.
Each should verify that calling `reset()` (or equivalent) then re-running the
same input produces bit-identical output to a fresh instance.

## Acceptance criteria

- [ ] `DelayBuffer`: test that `reset()` + same input sequence = same output as
  a fresh `DelayBuffer`.
- [ ] `PolyDelayBuffer`: same check for the poly variant.
- [ ] `ThiranInterp`: same check.
- [ ] `PolyThiranInterp`: same check.
- [ ] `HalfbandInterpolator`: same check.
- [ ] `PeakWindow`: same check.
- [ ] Each test carries a `/// T7 — determinism and state reset` doc comment
  per ADR 0022 convention.
- [ ] `cargo test -p patches-dsp` passes, `cargo clippy` clean.

## Notes

These tests live in the `#[cfg(test)]` modules of the respective source files
in `patches-dsp`. No new structs or public API are needed — this is purely
additive test coverage.

ADR 0022 technique reference: **T7 — Determinism and state reset**.
