---
id: "0210"
title: Extract SVF kernel to patches-dsp and add tests
priority: medium
created: 2026-03-30
---

## Summary

The Chamberlin State-Variable Filter (SVF) coefficients are currently computed
inline inside `patches-modules/src/svf.rs` and `poly_svf.rs` with no standalone
kernel struct. This makes the SVF untestable independently of the module harness.
This ticket extracts the coefficient computation and per-sample state update into a
standalone `SvfKernel` (or equivalent) type in `patches-dsp` and adds rigorous
tests.

## Acceptance criteria

- [ ] A `SvfKernel` struct (or equivalent — exact naming at implementor's discretion)
      in `patches-dsp` that encapsulates:
      - Coefficient computation from cutoff (Hz), sample rate, and Q.
      - Per-sample state update yielding lowpass, highpass, and bandpass outputs.
- [ ] A `PolySvfKernel` (or equivalent) 16-voice SIMD-friendly variant, mirroring
      the existing `PolyBiquad` pattern.
- [ ] `patches-modules/src/svf.rs` and `poly_svf.rs` refactored to use the new
      types from `patches-dsp`; existing module behaviour unchanged.
- [ ] **T1 — Impulse response:** Process a unit impulse through a known SVF setting
      and assert the output sequence matches a reference within tolerance 1e-9 for
      the lowpass output.
- [ ] **T2 — Frequency response:** For lowpass, highpass, and bandpass modes at a
      known Fc and Q, drive with spot-frequency sinusoids and assert steady-state
      amplitude is within ±1 dB of the theoretical transfer function.
- [ ] **T3 — DC:** Assert lowpass passes DC ≈ 1.0; highpass ≈ 0.0 at DC.
- [ ] **T3 — Nyquist:** Assert highpass passes Nyquist at ≈ 1.0.
- [ ] **T4 — Stability:** Run SVF at high resonance (Q = 10) for 10,000 samples;
      assert output is bounded.
- [ ] **T7 — Determinism:** Same input twice with state reset → bit-identical output.
- [ ] Existing `voct_shifts_filter_frequency` module test still passes.
- [ ] `cargo test` (workspace) passes; `cargo clippy` clean.

## Notes

Technique references (ADR 0022): T1, T2, T3, T4, T7.

Design note: The Chamberlin SVF computes `hp = input - lp - q*bp`, `bp += f*hp`,
`lp += f*bp`. Coefficients `f` (frequency) and `q` (damping) can be encapsulated
in a `SvfCoeffs` struct. The state is `(lp, bp)`. This split is clean and requires
no changes to the module-graph machinery.

The poly variant likely follows the same pattern as `PolyBiquad` — an array of
per-voice `SvfState` with coefficient broadcast/ramp update logic. If that ramp
logic already lives in `patches-dsp` post T-0207, reuse it.
