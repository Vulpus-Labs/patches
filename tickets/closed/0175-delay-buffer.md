---
id: "0175"
title: Circular delay buffer with Thiran interpolation
priority: medium
created: 2026-03-22
epic: "E030"
---

## Summary

Implements `DelayBuffer`, `ThiranInterp`, `PolyDelayBuffer`, and
`PolyThiranInterp` in `patches-modules/src/common/delay_buffer.rs` as the
shared primitive for delay, chorus, and FDN reverb modules.

## Acceptance criteria

- [x] `DelayBuffer::new(min_samples)` allocates the smallest power-of-two
      buffer ≥ `min_samples` using `next_power_of_two()`.
- [x] `DelayBuffer::for_duration(secs, sample_rate)` computes
      `ceil(secs * rate)` then calls `new`.
- [x] `DelayBuffer::push` advances write position then stores sample; `write`
      always points to the most recently written sample.
- [x] `read_nearest(offset: usize)` reads via bitmask wrap.
- [x] `read_linear(offset: f32)` — linear interpolation between floor and
      ceiling sample.
- [x] `read_cubic(offset: f32)` — Catmull-Rom cubic; guard taps wrap at
      `floor == 0`.
- [x] `ThiranInterp::read` — first-order Thiran all-pass; coefficient
      `a = (1 − η) / (1 + η)`; fractional part clamped to
      `[FRAC_EPSILON, 1 − FRAC_EPSILON]`.
- [x] `PolyDelayBuffer` stores interleaved `[f32; 16]` per time step.
- [x] `PolyThiranInterp::read` computes `a` once and applies across all 16
      voice states.
- [x] All four types exported from `patches_modules::common`.
- [x] Tests: push/read roundtrip, wrap-around, capacity rounding,
      `for_duration_at_48k`, linear midpoint, cubic partition-of-unity,
      Thiran DC pass-through; poly variants of read and DC test.
- [x] `cargo clippy`, `cargo test` pass with no warnings.

## Notes

`Box<[f32]>` chosen over `Vec<f32>`: capacity is fixed at construction, so
the extra word Vec carries is permanently wasted; `Box<[T]>` communicates the
fixed-size invariant.  Size is a runtime value (set from `AudioEnvironment` in
`prepare`), ruling out const-generic `[f32; N]`.

Poly layout rationale: interleaved `[f32; 16]` per time step means a read or
write for all voices at one offset touches one cache line.  Separate per-voice
buffers would scatter across 16 cache lines for every read/write operation.
