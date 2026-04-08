---
id: "0194"
title: "Register SAH/Quant modules and add integration tests"
priority: medium
created: 2026-03-25
epic: "E035"
---

## Summary

Register `Sah`, `PolySah`, `Quant`, and `PolyQuant` in `default_registry()` and
add integration tests covering the key behaviours of all four modules.

Depends on T-0190, T-0191, T-0192, T-0193.

## Acceptance criteria

### Registry

- [ ] `patches-modules/src/lib.rs` exports all four modules:

  ```rust
  pub mod sah;
  pub mod poly_sah;
  pub mod quant;
  pub mod poly_quant;
  pub use sah::Sah;
  pub use poly_sah::PolySah;
  pub use quant::Quant;
  pub use poly_quant::PolyQuant;
  ```

- [ ] `default_registry()` calls `r.register::<Sah>()`, `r.register::<PolySah>()`,
  `r.register::<Quant>()`, `r.register::<PolyQuant>()`.

### Integration tests

File: `patches-integration-tests/tests/sah_quant.rs`

- [ ] **`sah_holds_on_trigger`** — `Noise → Sah`, clock fires once at sample 1;
  verify `out` holds the same value for all subsequent samples.
- [ ] **`sah_updates_on_each_trigger`** — two triggers at samples 1 and 10;
  verify the held value changes between them (given sufficient noise variance).
- [ ] **`poly_sah_holds_all_voices`** — `PolyNoise → PolySah`, single trigger;
  verify all 16 voices are latched.
- [ ] **`quant_snaps_to_nearest_note`** — `Quant` with `notes = ["0", "7"]`
  (root and fifth); feed inputs at several known fractions; assert outputs
  are rounded to the expected semitone.
- [ ] **`quant_trig_out_fires_on_change`** — two different inputs fed on
  consecutive trigger cycles; assert `trig_out` goes high (≥ 0.5) exactly on
  the sample the pitch changes, and is 0.0 the next sample.
- [ ] **`quant_no_spurious_trig_out`** — same input on consecutive triggers;
  assert `trig_out` stays 0.0 after the initial quantise.
- [ ] **`quant_centre_and_scale`** — with `centre = 1.0`, `scale = 0.5`, and
  input 0.0, verify `out = 1.0 + (0.0 * 0.5) = 1.0`.
- [ ] **`poly_quant_per_voice_trig_out`** — two voices with different inputs
  that quantise to different notes; assert each voice's `trig_out` slot fires
  independently.

### General

- [ ] `cargo test` passes (all crates).
- [ ] `cargo clippy` passes with no warnings.

## Notes

Use the `HeadlessEngine` from `patches-integration-tests/src/lib.rs` for tests
that need plan activation. For unit-level port-wiring tests (e.g. verifying
`trig_out` pulse width), a small hand-rolled harness similar to those in
`connectivity_notification.rs` is fine.

For `sah_updates_on_each_trigger`, use a known pseudo-random noise source or
inject fixed values via a `ConstSource` test helper to avoid flakiness.
