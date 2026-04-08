---
id: "0206"
title: Add tests for fast_tanh
priority: high
created: 2026-03-30
---

## Summary

`fast_tanh` in `patches-modules/src/common/approximate.rs` is called on the audio
thread (biquad saturation path) but has no tests of any kind. This is the only
algorithm in the DSP stack with zero coverage. This ticket adds a minimal but
meaningful test suite.

## Acceptance criteria

- [ ] **T3 — Key points:** Assert `fast_tanh(0.0) ≈ 0.0`, `fast_tanh(x) → 1.0` as
      x → +∞, `fast_tanh(x) → −1.0` as x → −∞ (test at x = ±10.0 as a practical
      large-value proxy).
- [ ] **T5 — Antisymmetry:** Assert `fast_tanh(−x) ≈ −fast_tanh(x)` for a range of
      positive x values.
- [ ] **T6 — Accuracy:** Measure RMS error vs. `f64::tanh()` over the range [−3, 3]
      (where the approximation is in use). Assert error < some documented tolerance.
- [ ] **T8 — Monotonicity:** Assert output is non-decreasing over a sampled range
      (tanh is strictly monotone; an approximation bug can introduce local non-monotonicity).
- [ ] All tests live in `patches-modules/src/common/approximate.rs` under the existing
      `#[cfg(test)]` block, alongside the `fast_sine`/`fast_exp2` tests.
- [ ] `cargo test -p patches-modules` passes; `cargo clippy -p patches-modules` clean.

## Notes

Technique references (ADR 0022): T3, T5, T6, T8.

Note: this ticket adds tests in-place in `patches-modules`. Moving
`approximate.rs` to `patches-dsp` is a separate ticket (T-0208) that can be
done before or after this one.
