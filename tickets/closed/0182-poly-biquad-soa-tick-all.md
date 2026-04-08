---
id: "0182"
title: SoA layout for PolyBiquad + tick_all
priority: medium
created: 2026-03-23
---

## Summary

`PolyBiquad` currently uses an Array-of-Structs layout (`[VoiceFilter; 16]`) where each
voice's s1/s2 state variables are 48 bytes apart.  The inner loop calls `tick_voice(i, …)`
once per voice in sequence, preventing SIMD vectorisation across voices.

Reshape to Structure-of-Arrays: hoist every field to `[f32; 16]` directly on `PolyBiquad`.
Replace `tick_voice` with `tick_all(x: &[f32; 16], saturate: bool, ramp: bool) -> [f32; 16]`
which processes all 16 voices together in separate per-step loops, enabling auto-vectorisation.
Update the three callers in `poly_filter.rs`.

## Acceptance criteria

- [x] `VoiceFilter` struct removed; `PolyBiquad` fields are `[f32; 16]` arrays
- [x] `tick_all` added, `tick_voice` removed
- [x] `begin_ramp_voice`, `set_static`, `new_static`, `has_cv` unchanged in interface
- [x] All existing `poly_biquad` unit tests pass with updated field access
- [x] `poly_filter.rs` callers updated to call `tick_all` with `&audio` array
- [x] `cargo clippy` clean, `cargo test -p patches-modules` passes

## Notes

Inner loop structure for the no-saturation path (each loop is independently vectorisable):
```rust
// Step 1: y = b0*x + s1
let mut y = [0.0f32; 16];
for i in 0..16 { y[i] = self.b0[i] * x[i] + self.s1[i]; }
// Step 2: new_s1 = b1*x - a1*y + s2  (reads old s2, writes s1 — no cross-iter dep)
for i in 0..16 { self.s1[i] = self.b1[i] * x[i] - self.a1[i] * y[i] + self.s2[i]; }
// Step 3: new_s2 = b2*x - a2*y
for i in 0..16 { self.s2[i] = self.b2[i] * x[i] - self.a2[i] * y[i]; }
// Step 4 (ramp only): advance coefficients
for i in 0..16 { self.b0[i] += self.db0[i]; /* … */ }
```
For the saturation path, `fast_tanh` is a pure rational polynomial and IS vectorised
by LLVM as `fdiv.4s` on ARM NEON (confirmed in assembly). The same split structure
is used: compute `fb: [f32; 16]` via `from_fn(|i| fast_tanh(y[i]))`, then separate
loops for s1 and s2. The saturate path remains ~2× slower than the linear path due
to `fdiv.4s` throughput being ~3-4× lower than `fmul.4s`, not scalar execution.
