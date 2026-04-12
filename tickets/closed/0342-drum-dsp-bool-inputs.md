---
id: "0342"
title: Refactor DecayEnvelope, PitchSweep, BurstNoise to accept bool triggers
priority: high
created: 2026-04-12
---

## Summary

`DecayEnvelope`, `PitchSweep`, and `BurstNoise` in `patches-dsp/src/drum.rs`
all duplicate the `prev_trigger` / rising-edge detection pattern internally.
Like the `AdsrCore` change in ticket 0336, these should accept a `bool`
trigger parameter and let the calling module handle edge detection via
`TriggerInput`.

## Acceptance criteria

- [x] `DecayEnvelope::tick` takes `triggered: bool` instead of `trigger: f32`
- [x] `PitchSweep::tick` takes `triggered: bool` instead of `trigger: f32`
- [x] `BurstNoise::tick` takes `triggered: bool` instead of `trigger: f32`
- [x] `prev_trigger` fields removed from all three types
- [x] `reset()` methods updated (no longer need to clear `prev_trigger`)
- [x] All existing tests updated and passing
- [x] `cargo test -p patches-dsp` passes
- [x] `cargo clippy -p patches-dsp` clean

## Notes

Depends on 0335. Must be done before or alongside 0338 (drum module
refactoring), since the drum modules pass the raw trigger value into these
DSP types. After this change, drum modules pass the `bool` from
`TriggerInput::tick()` instead. See ADR 0030. Epic E062.
