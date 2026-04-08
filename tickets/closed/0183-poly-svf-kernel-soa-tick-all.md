---
id: "0183"
title: SoA layout for PolySvfKernel + tick_all
priority: medium
created: 2026-03-23
---

## Summary

`PolySvfKernel` uses an Array-of-Structs layout (`[VoiceSvf; 16]`) and calls
`tick_voice(i, x, ramp)` per voice sequentially, preventing SIMD vectorisation.

Reshape to Structure-of-Arrays: hoist `f_coeff`, `df`, `q_damp`, `dq`, `lp_state`,
`bp_state` to `[f32; 16]` arrays.  Add `tick_all(x: &[f32; 16], ramp: bool)`
returning `([f32; 16], [f32; 16], [f32; 16])` (lp, hp, bp).  Update `PolySvf::process`.

## Acceptance criteria

- [x] `VoiceSvf` struct removed; `PolySvfKernel` fields are `[f32; 16]` arrays
- [x] `tick_all` added, `tick_voice` removed
- [x] `begin_ramp_voice`, `set_static`, `new_static` interfaces unchanged
- [x] `PolySvf::process` updated to call `tick_all` and unpack the triple
- [x] `cargo clippy` clean, `cargo test -p patches-modules` passes

## Notes

SVF inner loop per step (each step is independently vectorisable):
```rust
// Step 1: lp = lp_state + f_coeff * bp_state
let lp: [f32; 16] = std::array::from_fn(|i|
    self.lp_state[i] + self.f_coeff[i] * self.bp_state[i]);
// Step 2: hp = x - lp - q_damp * bp_state  (depends on lp array, not lp[i±1])
let hp: [f32; 16] = std::array::from_fn(|i|
    x[i] - lp[i] - self.q_damp[i] * self.bp_state[i]);
// Step 3: bp = bp_state + f_coeff * hp
let bp: [f32; 16] = std::array::from_fn(|i|
    self.bp_state[i] + self.f_coeff[i] * hp[i]);
// State update
self.lp_state = lp;
self.bp_state = bp;
// Step 4 (ramp only): advance f_coeff, q_damp
```
Chain depth stays 3, but all 16 voices now execute each step together, enabling AVX2
(8 f32 per instruction) to halve the iteration count for each step.
