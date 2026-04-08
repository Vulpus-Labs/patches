---
id: "0216"
title: Add T6 SNR tests for MonoBiquad and SvfKernel
priority: low
created: 2026-03-30
---

## Summary

Both `MonoBiquad` and `SvfKernel` are now in `patches-dsp` (moved in E039),
which unblocks T6 (SNR / precision) tests. These tests verify that single-
precision recursive filter arithmetic stays within acceptable numerical error
bounds relative to an f64 reference, which is important for IIR filters where
rounding error accumulates.

## Acceptance criteria

- [ ] `biquad.rs`: T6 test processes a long sinusoid at Fc/10 through
  `MonoBiquad` (f32) and a reference biquad (f64); verifies SNR ≥ 60 dB.
- [ ] `svf.rs`: equivalent T6 test for `SvfKernel`.
- [ ] Each test carries a `/// T6 — SNR and precision` doc comment per ADR
  0022 convention.
- [ ] `cargo test -p patches-dsp` passes, `cargo clippy` clean.

## Notes

The f64 reference implementation can be a simple inline Transposed Direct
Form II / Chamberlin accumulation — no need for a separate struct. Compare
output arrays using RMS error.

ADR 0022 technique reference: **T6** — SNR and precision.
