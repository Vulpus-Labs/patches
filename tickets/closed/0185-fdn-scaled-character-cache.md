---
id: "0185"
title: Introduce ScaledCharacter to cache SR-scaled values in FdnReverb
priority: low
created: 2026-03-24
---

## Summary

`FdnReverb::process` recomputes several values every sample that are only
valid to recompute when the character or sample rate changes. Specifically:
`sr_ms` (a constant after `prepare`), `c.lfo_depth_ms * sr_ms`,
`c.max_pre_delay_ms * sr_ms`, and `BASE_MS[i] * sr_ms` (the per-line base
delay in samples before scale is applied). Introduce a `ScaledCharacter`
struct that pre-applies sample-rate-dependent scaling so `process` only
performs the multiplications that genuinely vary per sample.

## Acceptance criteria

- [ ] `ScaledCharacter` struct defined with fields:
  - `lfo_depth_samp: f32` — `c.lfo_depth_ms * sr_ms`
  - `max_pre_delay_samp: f32` — `c.max_pre_delay_ms * sr_ms`
  - `base_samps: [f32; LINES]` — `BASE_MS[i] * sr_ms` for each line
- [ ] `lfo_inc` is **not** a field of `ScaledCharacter`; the LFO phase
  accumulators remain the canonical store. The increment is only recomputed
  and written to the accumulators when character actually changes (same
  trigger as today in `update_validated_parameters`).
- [ ] `FdnReverb` holds a `ScaledCharacter` field, populated once in
  `prepare` and rebuilt in `update_validated_parameters` when character
  changes (SR is fixed post-`prepare` so no other trigger is needed).
- [ ] `process` uses `sc.lfo_depth_samp`, `sc.max_pre_delay_samp`, and
  `sc.base_samps[i]` directly; `sr_ms` is no longer computed in `process`.
- [ ] `BASE_MS[i] * scale` remains the per-sample multiplication (scale still
  varies with CV); only the `* sr_ms` factor is folded into `base_samps`.
- [ ] `cargo clippy` and `cargo test -p patches-modules` pass with no
  warnings.

## Notes

The existing `cached_scale` / `last_eff_size` / `last_eff_bright` mechanism
(T-0179) is orthogonal and unchanged.

`sr_ms` can be removed entirely from `process`; it is only needed when
building `ScaledCharacter` (in `prepare` and `update_validated_parameters`).
It does not need to be stored as a struct field — compute it inline at those
two call sites from `self.sample_rate`.
