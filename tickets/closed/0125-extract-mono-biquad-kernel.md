---
id: "0125"
title: Extract `MonoBiquad` kernel from `ResonantLowpass`
priority: high
created: 2026-03-18
epic: "E024"
depends_on: []
---

## Summary

The coefficient storage, TDFII recurrence, coefficient interpolation, and
snap/ramp mechanism currently embedded in `ResonantLowpass` are factored out
into a `MonoBiquad` struct in `patches-modules/src/common/mono_biquad.rs`.
`ResonantLowpass` is then rewritten as a thin wrapper that owns a `MonoBiquad`,
provides the coefficient formula, and manages port fields and parameters.

This does not change any observable behaviour — it is a pure refactor validated
by the existing test suite.

## Acceptance criteria

- [ ] `patches-modules/src/common/mono_biquad.rs` defines `pub struct MonoBiquad`
      with the following fields (all `f32`, all private):
      - Active coefficients: `b0`, `b1`, `b2`, `a1`, `a2`
      - Target coefficients: `b0t`, `b1t`, `b2t`, `a1t`, `a2t`
      - Per-sample deltas: `db0`, `db1`, `db2`, `da1`, `da2`
      - Filter memory: `s1`, `s2`
      - Update counter: `update_counter: u32` (also private)
- [ ] `MonoBiquad::new(b0, b1, b2, a1, a2) -> Self` initialises active and
      target to the given values, zeros all deltas and state, and sets
      `update_counter = 0`.
- [ ] `MonoBiquad::set_static(b0, b1, b2, a1, a2)` writes the given values
      into both active and target slots and zeros all deltas. Does not touch
      `s1`/`s2` or `update_counter`. Used when no CV is connected or when
      parameters change on the static path.
- [ ] `MonoBiquad::should_update(&self) -> bool` returns `true` when
      `update_counter == 0`.
- [ ] `MonoBiquad::begin_ramp(&mut self, b0t, b1t, b2t, a1t, a2t)` snaps
      active coefficients to the current targets (eliminating accumulated delta
      drift), stores the new targets, and computes per-sample deltas as
      `(target - active) * COEFF_UPDATE_INTERVAL_RECIPROCAL`. Called by the
      owning module after computing new target coefficients from live CV values.
- [ ] `MonoBiquad::tick(&mut self, x: f32, saturate: bool) -> f32` runs one
      sample of the Transposed Direct Form II recurrence:
      - `y  = b0·x + s1`
      - `fb = if saturate { fast_tanh(y) } else { y }`
      - `s1 = b1·x − a1·fb + s2`
      - `s2 = b2·x − a2·fb`
      - advances active coefficients by their deltas
      - increments `update_counter`, wrapping at `COEFF_UPDATE_INTERVAL`
      - returns `y`
      Advancing deltas in `tick` keeps all hot state in one place; the owning
      module calls `should_update` before `tick` to decide whether to recompute
      targets.
- [ ] `COEFF_UPDATE_INTERVAL` (32) and `COEFF_UPDATE_INTERVAL_RECIPROCAL`
      are `pub(crate)` constants in `mono_biquad.rs`, re-exported from
      `common::mod` for use by `poly_biquad` (T-0126).
- [ ] `MonoBiquad` is re-exported from `crate::common`.
- [ ] `ResonantLowpass` is rewritten to hold `biquad: MonoBiquad` in place of
      all inline coefficient and state fields. Its `process` implementation
      calls `biquad.should_update()`, `biquad.begin_ramp(...)`, and
      `biquad.tick(x, self.saturate)`.
- [ ] `recompute_static_coeffs` on `ResonantLowpass` calls
      `self.biquad.set_static(...)` with coefficients from
      `compute_biquad_lowpass`.
- [ ] All existing tests in `patches-modules/src/filter.rs` pass without
      modification to their assertions.
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no new warnings.

## Notes

`compute_biquad_lowpass` and `resonance_to_q` remain as module-level free
functions in `filter.rs`; they are not part of the kernel. Each filter topology
provides its own coefficient formula.

`fast_tanh` is called from within `MonoBiquad::tick`. The import lives in
`mono_biquad.rs`.

`COEFF_UPDATE_INTERVAL` is 32 throughout this epic; do not make it a
constructor parameter — if it ever needs to be tunable that is a separate
design decision.
