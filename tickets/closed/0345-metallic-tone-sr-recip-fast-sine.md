---
id: "0345"
title: Cache sr_recip and use fast_sine in MetallicTone
priority: medium
created: 2026-04-12
---

## Summary

`MetallicTone::tick_with_modulation` computes `1.0 / self.sample_rate`
every sample and calls `(mod_phase * TAU).sin()` for the modulation signal.
Both are unnecessary work.

## Changes

- Add `sr_recip: f32` field, computed once in `new()`.
- Replace `(mod_phase * TAU).sin()` with `fast_sine(mod_phase)` — the
  phase is already in `[0, 1)` which is exactly `fast_sine`'s input
  convention. The modulation LFO doesn't need high-precision sine.
- Hoist `mod_signal * mod_depth * sr_recip` out of the per-partial loop
  (constant across all six partials).

## Acceptance criteria

- [ ] `sr_recip` stored in struct, computed in `new()`
- [ ] `tick_with_modulation` uses `fast_sine(mod_phase)` instead of `(mod_phase * TAU).sin()`
- [ ] `mod_signal * mod_depth * sr_recip` computed once before the loop
- [ ] `cargo test -p patches-dsp` passes
- [ ] `cargo clippy -p patches-dsp` clean

## Notes

Epic E063.
