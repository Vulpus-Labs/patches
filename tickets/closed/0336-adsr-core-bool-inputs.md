---
id: "0336"
title: Refactor AdsrCore to accept bools instead of floats
priority: high
created: 2026-04-12
---

## Summary

Change `AdsrCore::tick` from `tick(trigger: f32, gate: f32)` to
`tick(triggered: bool, gate_high: bool)`. Remove the internal `prev_trigger`
field and edge-detection logic. Edge detection moves to the calling module
layer via the new `TriggerInput`/`GateInput` types.

## Acceptance criteria

- [ ] `AdsrCore::tick` takes `(triggered: bool, gate_high: bool)`
- [ ] `prev_trigger` field removed from `AdsrCore`
- [ ] Internal edge detection replaced with direct use of the `triggered` bool
- [ ] Gate check (`gate < 0.5`) replaced with `!gate_high`
- [ ] All existing `AdsrCore` tests updated and passing
- [ ] `cargo test -p patches-dsp` passes
- [ ] `cargo clippy -p patches-dsp` clean

## Notes

Depends on 0335 (types must exist before callers can be updated, but
AdsrCore itself has no dependency on patches-core). See ADR 0030. Epic E062.
