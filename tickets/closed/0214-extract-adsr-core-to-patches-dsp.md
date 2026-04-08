---
id: "0214"
title: Extract ADSR core ramp logic to patches-dsp
priority: medium
created: 2026-03-30
---

## Summary

The ADSR state machine (Idle → Attack → Decay → Sustain → Release), linear
ramp arithmetic, and retrigger logic are embedded in `patches-modules`
(`adsr.rs`, `poly_adsr.rs`). Per ADR 0022 these belong in `patches-dsp` so
they can be tested independently of the module harness.

The ramp-slope tests currently in `patches-modules` are DSP tests (asserting
per-sample accuracy), not protocol tests. Moving the core enables proper T4,
T5, and T7 coverage and reduces module tests to gate/trigger wiring.

## Acceptance criteria

- [ ] `patches-dsp/src/adsr.rs` contains a standalone `AdsrCore` (or
  equivalent) with state machine and linear ramp logic, no dependency on
  `patches-core` or `patches-modules`.
- [ ] T4 test: rapid gate toggling (dozens of toggles per stage) does not
  produce NaN, infinity, or values outside [0, 1].
- [ ] T5 test: attack, decay, and release ramps are linear (per-sample slope
  is constant within floating-point tolerance).
- [ ] T7 test: resetting state and rerunning the same gate sequence produces
  bit-identical output.
- [ ] `patches-modules` `adsr.rs` / `poly_adsr.rs` updated to import core from
  `patches-dsp`.
- [ ] Ramp-slope tests removed from `patches-modules` (now covered by
  `patches-dsp`). Gate/trigger and voice-independence tests remain.
- [ ] `cargo test` and `cargo clippy` pass across the workspace.

## Notes

If exponential curve shaping is added later, extend T5 to verify the shape
function's properties at that time.

ADR 0022 technique references: **T4** (stability), **T5** (linearity),
**T7** (determinism and state reset).
