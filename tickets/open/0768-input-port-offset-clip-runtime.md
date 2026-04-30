---
id: "0768"
title: Input port offset + clip runtime
priority: medium
created: 2026-04-30
epics: ["E128"]
adrs: ["0062"]
---

## Summary

Extend `MonoInput`, `PolyInput`, and `StereoInput` in
`patches-core/src/cables/` with `offset: f32` and
`clip: Option<(f32, f32)>`. Update `read` to apply
`v * scale + offset` then optional clamp. Pure-scalar cables keep
the existing fast path: `offset = 0.0` and `clip = None` mean a
single `mul` and a branch-predictable `None` arm.

## Acceptance criteria

- [ ] All three input ports gain `offset` and `clip` fields, with
      `Default` initialising them to `0.0` and `None`.
- [ ] `read` for each port type applies the affine then conditional
      clamp. `PolyInput::read` and `StereoInput::read` apply the
      same affine + clip to every channel.
- [ ] Microbench (`patches-core` benches dir, behind a feature
      gate if needed) on `MonoInput::read` shows no statistically
      significant regression on the scalar fast path vs. the
      pre-change baseline. Acceptable noise threshold: ±1%.
- [ ] All existing struct literals (call-sites, tests) updated. A
      `MonoInput::scalar(cable_idx, scale)` constructor avoids
      churn in tests that don't care about the new fields.
- [ ] `cargo test -p patches-core -p patches-modules -p patches-dsp -p patches-engine`
      and `cargo clippy` pass.

## Notes

Reference: [ADR 0062 §Builder lowering and runtime application](../../adr/0062-cable-range-expressions.md).
This is the visible runtime change. It lands while the lowering
path (0769) still produces `offset = 0`, `clip = None` for every
cable, so behaviour is unchanged until the next ticket flips the
switch.
