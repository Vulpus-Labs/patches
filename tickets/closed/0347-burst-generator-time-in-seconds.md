---
id: "0347"
title: BurstGenerator set_params should accept time in seconds
priority: low
created: 2026-04-12
---

## Summary

`BurstGenerator::set_params` takes `burst_spacing_samples: usize`, forcing
callers to pre-multiply by sample rate. All other drum DSP types
(`DecayEnvelope`, `PitchSweep`) store `sample_rate` and accept time in
seconds. `BurstGenerator` should follow the same convention for
consistency.

## Changes

- Add `sample_rate: f32` field to `BurstGenerator`, passed in `new(sample_rate)`.
- Change `set_params` to accept `burst_spacing_secs: f32` and convert
  internally.
- Update callers.

## Acceptance criteria

- [ ] `BurstGenerator::new` takes `sample_rate`
- [ ] `set_params` accepts spacing in seconds
- [ ] Callers updated
- [ ] `cargo test -p patches-dsp` passes
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy` clean

## Notes

Epic E063. Low priority — consistency improvement, not a performance fix.
