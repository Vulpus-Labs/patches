---
id: "0344"
title: Separate MetallicTone configuration from triggering
priority: medium
created: 2026-04-12
---

## Summary

`MetallicTone::trigger(base_hz)` calls `set_frequency(base_hz)` on every
trigger, computing six multiply-divides for the partial increments. But
`base_hz` only changes on patch reload — it's the same value on every hit.

`trigger()` should just reset phases; frequency should only be set when
params change.

## Changes

- `trigger()` takes no arguments, just calls `reset()` (resets phases).
- Callers (hihat, cymbal) call `set_frequency` in `set_parameters` and
  `trigger()` in the trigger block.

## Acceptance criteria

- [ ] `MetallicTone::trigger()` takes no arguments, resets phases only
- [ ] Hihat and cymbal modules call `set_frequency` in `set_parameters`
- [ ] Hihat and cymbal trigger blocks call `trigger()` without `base_hz`
- [ ] `cargo test -p patches-dsp` passes
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy` clean

## Notes

Epic E063.
