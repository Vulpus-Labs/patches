---
id: "0207"
title: Move biquad kernel to patches-dsp and add independent tests
priority: high
created: 2026-03-30
---

## Summary

`patches-modules/src/common/mono_biquad.rs` and `poly_biquad.rs` are pure-algorithm
types with no dependency on the module harness, `CablePool`, or any `patches-core`
type. Moving them to `patches-dsp` enables rigorous independent tests of the biquad
transfer function and decouples filter correctness from module-wiring tests.

This is the highest-impact structural change in the E039 epic: 25 module-level filter
tests (`filter.rs`, `poly_filter.rs`) currently conflate DSP correctness with port
wiring. Once the biquad kernel has its own tests in `patches-dsp`, those module tests
can be narrowed to wiring and protocol verification.

## Acceptance criteria

- [ ] `MonoBiquad` and `PolyBiquad` (including coefficient-ramp logic) moved to
      `patches-dsp/src/biquad.rs` (or split into `mono_biquad.rs` / `poly_biquad.rs`).
- [ ] `patches-dsp` re-exports the types at its crate root or a named module.
- [ ] `patches-modules` imports them from `patches-dsp`; no duplication.
- [ ] **T1 — Impulse response:** Feed a unit impulse through a known biquad
      (e.g. Butterworth lowpass at Fc = 0.1·fs) and assert the output sequence
      matches a reference computed analytically or via a trusted implementation,
      within tolerance 1e-9.
- [ ] **T2 — Frequency response:** For lowpass, highpass, and bandpass coefficient
      sets, drive with sinusoids at several spot frequencies and assert steady-state
      amplitude is within ±0.5 dB of the theoretical transfer function value.
- [ ] **T3 — DC:** Assert lowpass passes DC at unity; highpass attenuates DC to ~0;
      bandpass attenuates DC.
- [ ] **T3 — Nyquist:** Assert highpass passes Nyquist at near-unity; lowpass
      attenuates Nyquist.
- [ ] **T4 — Stability:** Run a biquad with high-resonance coefficients (Q ≈ 10) for
      10,000 samples of white-noise-amplitude sine; assert output remains bounded
      (no NaN/infinity, |output| < 1000).
- [ ] **T7 — Determinism:** Process the same input sequence twice with a state reset
      between runs; assert outputs are bit-identical.
- [ ] Existing unit tests in `poly_biquad.rs` (coefficient broadcast, ramp
      interpolation, voice independence) are migrated to the new location.
- [ ] All 4 existing poly_biquad tests still pass in their new home.
- [ ] `cargo test` (workspace) passes; `cargo clippy` clean.

## Notes

Technique references (ADR 0022): T1, T2, T3, T4, T7.

**Scope boundary:** This ticket moves the kernel and adds DSP tests. Simplifying
the 25 module-level filter tests in `filter.rs` / `poly_filter.rs` to be
protocol-only is _not_ in scope here — that is a follow-on task once the kernel
has independent coverage.

The coefficient-update machinery (`set_static`, `begin_ramp`, `tick_all`) should
move with the kernel; it is a detail of how the biquad accepts smoothed coefficient
updates and has no module-harness dependencies.
