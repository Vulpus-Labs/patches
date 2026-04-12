---
id: "0343"
title: Separate PitchSweep configuration from triggering
priority: medium
created: 2026-04-12
---

## Summary

`PitchSweep::set_params` computes an `exp()` to derive `sweep_coeff`, and
is called on every trigger. But the parameters (`start_hz`, `end_hz`,
`sweep_time_secs`) only change on patch reload via `set_parameters` — they
are identical on every trigger. The `exp()` should only be computed when
params actually change.

## Changes

- Add a `start_hz` field to `PitchSweep` so the start frequency is
  remembered across triggers.
- `set_params` stores `start_hz`, `end_hz`, and computes `sweep_coeff`.
  Does not reset `current_hz` (configuration only).
- `trigger()` becomes `self.current_hz = self.start_hz` — no `exp()`, no
  parameters.
- Drum module `set_parameters` methods call `set_params` (as they already
  do).
- Drum module `if trigger_rose` blocks call `self.pitch_sweep.trigger()`
  instead of `self.pitch_sweep.set_params(...)`.

## Acceptance criteria

- [ ] `PitchSweep` stores `start_hz`
- [ ] `set_params` computes coefficient only, does not reset `current_hz`
- [ ] `trigger()` takes no arguments, resets `current_hz` to `start_hz`
- [ ] All drum modules updated: `set_params` in `set_parameters`, `trigger()` in trigger block
- [ ] `cargo test -p patches-dsp` passes
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy` clean

## Notes

Similar separation of "configure" vs "fire" could apply to `DecayEnvelope`
(`set_decay` is already separate from `tick`, so it's fine) and
`BurstNoise`. Epic E063.
