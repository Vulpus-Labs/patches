---
id: "0970"
title: Q32 phase accumulator types in patches-dsp
priority: high
created: 2026-05-29
---

## Summary

Add `MonoPhaseAccumulatorQ32` and `PolyPhaseAccumulatorQ32` to `patches-dsp`,
beside the existing f32 `MonoPhaseAccumulator` / `PolyPhaseAccumulator`. These
hold a `u32` Q32 phase (full `u32` range = one cycle, free wrap via
`wrapping_add`) and a `u32` per-sample increment, mirroring the hypersaw form
(ADR 0078) but as a reusable per-voice accumulator rather than a voice-batched
kernel. They are the foundation for the LFO and Op migrations (E159, ADR 0080).

## Acceptance criteria

- [x] `MonoPhaseAccumulatorQ32`: `phase: u32`, `increment: u32`;
      `set_increment(f32 cycles/sample)` clamps and scales to `u32`
      (`(frac * 2^32) as u32`); `advance()` = `wrapping_add`; `reset()`;
      `sync_reset(frac: f32)` for trigger-driven reset; a `u32` phase read and a
      `[0,1)` f32 helper.
- [x] `PolyPhaseAccumulatorQ32`: `[u32; 16]` phase + increment; `advance_all()`
      over the fixed-16 array (autovec-friendly, fixed trip count);
      `set_increment(voice, f32)`, `set_all_increments(f32)`, `reset(voice)`,
      `reset_all()`, `sync_reset(voice, frac)`.
- [x] Both exported from `patches-dsp` lib root next to the f32 types.
- [x] Tests: determinism (same increment → bit-identical phase sequence);
      free-wrap correctness (phase wraps at `2^32` with no conditional);
      **low-rate no-absorption** (increment of a 0.001 Hz / 48 kHz tone advances
      the phase every sample — the f32 failure this exists to fix);
      `sync_reset` lands the documented sub-sample phase.
- [x] `lookup_sine_q32` (already in `patches-dsp::approximate`) confirmed
      exported and covered by a parity test against `lookup_sine` within table
      tolerance.
- [x] `just commit -p patches-dsp` green; `cargo clippy` clean.

## Notes

- ADR 0080 §1, Epic E159. Blocks 0971 and 0972.
- Do **not** retrofit `Oscillator`/`PolyOsc` onto these — they stay f32 (ADR
  0080 §3). This is additive; the f32 types remain.
- `set_increment` parity with the f32 path: the input is the same
  `frequency/sample_rate` cycles-per-sample value the f32 accumulator takes; only
  the storage representation differs. Q32 fixes *accumulation* rounding, not the
  f32-sourced increment precision (ADR 0080 Context).
- Reference fixed-point form: `patches-dsp/src/hypersaw.rs` (phase reinterpret,
  free wrap) and the new `lookup_sine_q32`.
