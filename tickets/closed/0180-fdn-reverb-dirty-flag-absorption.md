---
id: "0180"
title: "FdnReverb: dirty-flag absorption recomputation; skip when CV unconnected and params unchanged"
priority: medium
created: 2026-03-23
epic: "E031"
---

## Summary

`FdnReverb::process` calls `recompute_absorption` every `COEFF_UPDATE_INTERVAL`
(32) samples unconditionally ([fdn_reverb.rs:341–344](../../patches-modules/src/fdn_reverb.rs#L341)).
`recompute_absorption` calls `derive_params` (one `powf`) and then
`absorption_coeffs` eight times, each of which calls `10_f32.powf(...)` twice
plus `cos`, `sin`, and `sqrt` — roughly 18 transcendental calls per recompute,
or ~24 900 per second at 44 100 Hz.

The code comment acknowledges that when parameters are static, `begin_ramp`
receives identical targets and produces zero deltas, so the work is arithmetically
a no-op — but the expensive transcendental function calls still execute.

When neither `size_cv` nor `brightness_cv` is connected (the common case)
`eff_size` and `eff_bright` are constants.  The absorption coefficients should be
computed once — during `prepare` and on `update_validated_parameters` — and never
again until a parameter change arrives.  When CV _is_ connected the existing
32-sample cadence is retained so that modulated coefficients stay smooth.

## Acceptance criteria

- [ ] `FdnReverb` gains a `absorption_dirty: bool` field, initialised to `false`
  after `prepare` (which already computes the initial coefficients).
- [ ] `update_validated_parameters` sets `absorption_dirty = true` whenever
  `size_param`, `bright_param`, or `character` changes.
- [ ] `process` recomputes absorption when either:
  - `absorption_dirty` is true (set it back to `false` after recomputing), or
  - at least one CV input (`in_size_cv`, `in_brightness_cv`) is connected AND
    `coeff_counter` has reached `COEFF_UPDATE_INTERVAL`.
- [ ] When neither CV is connected and `absorption_dirty` is `false`, the
  `coeff_counter` is not incremented and `recompute_absorption` is not called.
- [ ] The `coeff_counter` is reset to 0 after a dirty-flag recompute so the
  32-sample interval restarts cleanly when CV is subsequently connected.
- [ ] `cargo test -p patches-modules` and `cargo clippy` pass with no new
  warnings.

## Notes

The `begin_ramp` zero-delta drift concern (cited in the existing comment) only
applies when active coefficients have been accumulating interpolation steps; if
the coefficients are never touched between parameter changes there is no drift to
correct.  The drift concern is therefore moot on the static path.

This ticket is independent of T-0178 (`PeriodicUpdate` trait).  T-0178 moves the
32-sample recomputation cadence from an in-`process()` counter to a dedicated
trait method; the dirty-flag gate added here is orthogonal and will continue to
work correctly if T-0178 is applied later (the dirty-flag check moves into
`periodic_update`, and the static-path skip in `process` is simply removed
because `periodic_update` is no longer called from `process`).

If T-0178 lands before this ticket, scope this ticket to adding the dirty-flag
gate inside `FdnReverb::periodic_update` instead.
