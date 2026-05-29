---
id: "0972"
title: Migrate Op + PolyOp to Q32 phase (perf + consistency)
priority: medium
created: 2026-05-29
---

## Summary

Move `Op` and `PolyOp` off the shared f32 `*PhaseAccumulator` to the Q32
accumulator (0970). The justification is **perf + consistency**, not pitch: a
bench (`patches-profiling`, `osc_fixedpoint_bench`) puts a PolyOp 2-op FM cell at
~72% of f32 cost under Q32 (~28% faster on the phase/PM/feedback/lookup core).
The FM win stacks — two `rem_euclid` wraps per voice (feedback smoother + carrier
PM) and two `lookup_sine` calls, all made cheaper by Q32 (free wrap +
`lookup_sine_q32`). Op mono is migrated alongside to keep the operator family on
one phase idiom.

## Acceptance criteria

- [x] `Op` and `PolyOp` use `MonoPhaseAccumulatorQ32` / `PolyPhaseAccumulatorQ32`.
- [x] The operator read phase `(phase + pm + fb_avg).rem_euclid(1.0)` becomes a
      `u32` `wrapping_add` of phase + `(pm * 2^32) as i64 as u32` +
      `(fb_avg * 2^32) as i64 as u32` (free wrap, no `rem_euclid`).
- [x] `op_waveform` Sine reads `lookup_sine_q32`; analytic shapes use `u32`
      threshold compares (`phase < 0.5`, etc.); the 2-sample rolling-average
      feedback smoother is unchanged (operates on the f32 offset before
      conversion).
- [x] `phase_reset` / `start_phase` apply via `sync_reset` (or direct `u32`
      phase set); per-voice trigger reset preserved.
- [x] Existing `Op` / `PolyOp` tests pass (phase-reset, voct independence,
      envelope independence, waveform shapes); tolerances retuned only where the
      representation legitimately shifts values.
- [x] Bench check: re-run `osc_fixedpoint_bench op` and confirm the Q32 PolyOp
      delta reproduces (Q32 materially below f32); record the numbers in the PR.
      Measured (aarch64, n=4M, 16 voices, 48 kHz): f32 82.85 ns/tick vs Q32
      58.21 ns/tick → **70.3% of f32** (~30% faster), reproducing ADR 0080's ~72%.
- [x] `just commit -p patches-modules` green; `cargo clippy` clean.

## Notes

- ADR 0080 §2, Epic E159. Depends on 0970. Goldens regenerated in 0973.
- Op's pitch case is weak (audio-rate f32 phase is adequate; Q32 does not improve
  tuning — increment is f32-sourced). This ticket is perf + avoiding a third
  phase representation, per ADR 0080.
- The bench's headline ~28% is the phase subsystem in isolation; the per-voice
  ADSR is port-invariant and dilutes the real-module percentage (~20–25%).
- The shared f32 `MonoPhaseAccumulator`/`PolyPhaseAccumulator` stay in place for
  `Oscillator`/`PolyOsc` (ADR 0080 §3); do not remove them.
