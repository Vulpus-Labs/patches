---
id: "0179"
title: "FdnReverb: cache derived scale, eliminate per-sample `derive_params` call"
priority: medium
created: 2026-03-23
epic: "E031"
---

## Summary

`FdnReverb::process` calls `derive_params(eff_size, eff_bright, self.character)`
every sample to obtain `scale` for the LFO delay-read offset calculation
([fdn_reverb.rs:364](../../patches-modules/src/fdn_reverb.rs#L364)).
`derive_params` contains an exponential interpolation (`powf`) that maps `size`
to a delay-line scale factor.  When neither `size_cv` nor `brightness_cv` is
connected (the common case) `eff_size` and `eff_bright` are constants, so
`scale` never changes between calls.  At 44 100 Hz this is ~44 000 wasted `powf`
calls per second.

The fix: store the derived parameters (`scale`, `rt60_lf`, `rt60_hf`,
`crossover`) in the `FdnReverb` struct and recompute them only when `eff_size`
or `eff_bright` actually changes.

## Acceptance criteria

- [ ] `FdnReverb` gains a `cached_scale: f32` field (and `cached_rt60_lf`,
  `cached_rt60_hf`, `cached_crossover: f32` if they are also used more than once
  per sample, or just the ones needed to avoid recomputation).
- [ ] A `last_eff_size: f32` and `last_eff_bright: f32` field (or a `params_dirty:
  bool` flag) gate recomputation of the derived parameters.
- [ ] The per-sample call to `derive_params` on the LFO path (currently line 364)
  is replaced with a read of `self.cached_scale`; `derive_params` is called only
  when `eff_size` or `eff_bright` differs from the cached values.
- [ ] The `recompute_absorption` path (which already calls `derive_params`
  internally) is unaffected by this ticket — it is addressed separately in
  T-0180.
- [ ] `cargo test -p patches-modules` and `cargo clippy` pass with no new
  warnings.

## Notes

Comparison to detect change should use an exact `!=` check on the `f32` values
(not a threshold) — a threshold might silently skip an update after a hot-reload
parameter change.  Since `eff_size` and `eff_bright` are already clamped to
[0, 1] there are no NaN or infinity concerns.

Initial value for `cached_scale` (and friends) should be set at the end of
`prepare`, after computing the initial absorption coefficients, so the first
`process` call does not trigger an unnecessary recomputation.
