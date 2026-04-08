---
id: "0208"
title: Move approximate.rs and waveforms.rs to patches-dsp
priority: medium
created: 2026-03-30
---

## Summary

`patches-modules/src/common/approximate.rs` (`fast_sine`, `fast_exp2`, `fast_tanh`)
and `patches-modules/src/common/waveforms.rs` (1024-point sine wavetable + linear
interpolation, poly variant) are pure numerical utilities with no dependency on
module harness types. Moving them to `patches-dsp` makes them first-class primitives,
findable by future modules and external code, and properly separated from module
concerns per ADR 0022.

## Acceptance criteria

- [ ] `approximate.rs` moved to `patches-dsp/src/approximate.rs`; all three
      functions (`fast_sine`, `fast_exp2`, `fast_tanh`) re-exported from
      `patches-dsp` crate root or a named module.
- [ ] `waveforms.rs` (wavetable + `lookup_sine`, `poly_lookup_sine`) moved to
      `patches-dsp/src/wavetable.rs` (or similar); re-exported from crate root.
- [ ] `patches-modules` imports from `patches-dsp`; no duplicated code.
- [ ] All existing tests for these files migrate to the new location and pass.
- [ ] **T6 — fast_sine SNR:** Assert RMS error of `fast_sine` vs `f64::sin()` across
      a dense sweep of the full period is below the documented tolerance (currently
      tested as < 0.01; assert this explicitly in the new location).
- [ ] **T6 — Wavetable SNR:** Assert RMS error of `lookup_sine` vs `f64::sin()`
      across a dense phase sweep is within an acceptable tolerance (document the
      tolerance).
- [ ] `cargo test` (workspace) passes; `cargo clippy` clean.

## Notes

Technique references (ADR 0022): T6, T8 (existing coverage).

The `fast_tanh` tests from T-0206 should land in the new `patches-dsp` location if
T-0206 is done first; otherwise they will be created here. Coordinate ordering
with T-0206 to avoid double-writing.
