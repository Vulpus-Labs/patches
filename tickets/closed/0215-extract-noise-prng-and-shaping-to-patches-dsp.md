---
id: "0215"
title: Extract noise PRNG and spectral shaping filters to patches-dsp
priority: medium
created: 2026-03-30
---

## Summary

The xorshift64 PRNG and the spectral shaping IIR filters (pink/brown/red) live
in `patches-modules/src/noise.rs`. Per ADR 0022 they belong in `patches-dsp`.
Moving them enables T7 (determinism — same seed → same sequence) and T10
(spectral density) tests independent of the module harness.

## Acceptance criteria

- [ ] `patches-dsp/src/noise.rs` contains the xorshift64 PRNG and spectral
  shaping filters, with no dependency on `patches-core` or `patches-modules`.
- [ ] T7 test: constructing two generators with the same seed and calling
  `next()` N times on each produces identical sequences.
- [ ] T8 test: all-zero seed either produces a defined non-stuck sequence or
  panics with a clear message documenting the precondition.
- [ ] T10 test: white noise has approximately flat power; pink noise rolls off
  at approximately −3 dB/octave (autocorrelation or binned-variance method;
  tolerance ±6 dB).
- [ ] `patches-modules/src/noise.rs` updated to import from `patches-dsp`.
- [ ] Spectral-smoothness test currently in `patches-modules` either removed
  or narrowed to module-protocol concerns.
- [ ] `cargo test` and `cargo clippy` pass across the workspace.

## Notes

The T10 spectral test can use a simple binned-variance approach (compare
variance of low-frequency vs. high-frequency bins) rather than a full FFT,
which avoids adding an FFT dependency. Document the approach in the test with
the ADR 0022 technique reference.

ADR 0022 technique references: **T7** (determinism), **T8** (edge-case
inputs), **T10** (statistical / perceptual properties).
