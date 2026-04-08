---
id: "0213"
title: Extract oscillator phase accumulator and PolyBLEP to patches-dsp
priority: medium
created: 2026-03-30
---

## Summary

The oscillator phase accumulator and PolyBLEP anti-aliasing corrections are
currently embedded in `patches-modules` (`oscillator.rs`, `lfo.rs`,
`poly_osc.rs`, `common/phase_accumulator.rs`). Per ADR 0022, pure DSP logic
belongs in `patches-dsp`. Moving it enables independent T2 (THD / spectral
analysis) and T7 (phase-reset determinism) tests and reduces module tests to
protocol concerns only.

## Acceptance criteria

- [ ] `patches-dsp/src/oscillator.rs` contains a standalone
  `PhaseAccumulator` struct and PolyBLEP correction functions (or equivalent
  API) with no dependency on `patches-core` or `patches-modules`.
- [ ] T2 test: spectral purity of a generated waveform at a spot frequency
  (e.g. verify that PolyBLEP reduces high-frequency aliasing compared to naive
  accumulation).
- [ ] T7 test: resetting phase and rerunning produces bit-identical output.
- [ ] `patches-modules` modules (`oscillator.rs`, `lfo.rs`, `poly_osc.rs`)
  are updated to import from `patches-dsp`.
- [ ] Waveform-correctness tests in `patches-modules` (period checks, waveform
  formula checks) are either removed (if now covered by `patches-dsp` tests)
  or narrowed to protocol / parameter-dispatch concerns.
- [ ] `cargo test` and `cargo clippy` pass across the workspace.

## Notes

The `common/phase_accumulator.rs` file may be left as a re-export shim (like
`common/approximate.rs` after E039) to avoid breaking internal callers.

ADR 0022 technique references: **T2** (frequency response), **T7**
(determinism and state reset).
