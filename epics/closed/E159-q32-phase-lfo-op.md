---
id: E159
title: Q32 fixed-point phase migration — LFO and Op families
status: closed
created: 2026-05-29
---

## Goal

Extend the `u32` Q32 phase accumulator (ADR 0078, hypersaw) to the **modulation**
oscillators: `Lfo` / `PolyLfo` and `Op` / `PolyOp`. Design and rationale in
**ADR 0080**. Two motivations, measured:

- **LFO correctness.** At the bottom of the LFO's own rate clamp (0.001 Hz) the
  f32 per-sample increment (~2e-8) falls below the f32 ulp near `phase = 1.0`
  (~6e-8) and is absorbed — the LFO stalls / steps unevenly. Q32 holds uniform
  32-bit resolution and never absorbs the increment.
- **Op perf.** A bench (`patches-profiling`, `osc_fixedpoint_bench`) shows a
  PolyOp 2-op FM cell at ~72% of f32 cost under Q32 (~28% faster): two
  `rem_euclid` wraps and two `lookup_sine` calls per voice both get cheaper
  (free wrap + `lookup_sine_q32`). Op is migrated for this + consistency, not
  pitch.

Key decisions (ADR 0080):

- **New Q32 accumulator types** (`MonoPhaseAccumulatorQ32`,
  `PolyPhaseAccumulatorQ32`) live beside the f32 ones; `lookup_sine_q32` is the
  table reader (already landed).
- **Oscillator/PolyOsc stay f32** — bench shows parity on sine/saw, a square win
  only via a branchless-BLEP rewrite, and a regression on hard-sync. Out of
  scope, gated on a separate branchless-BLEP effort.
- **Q32 fixes accumulation, not increment precision.** Tuning is unchanged
  (increment still f32-sourced); better tuning is an orthogonal f64 change, not
  in this epic.

## Scope

**In:**

- `MonoPhaseAccumulatorQ32` / `PolyPhaseAccumulatorQ32` in `patches-dsp`:
  `u32` phase + increment, free wrap, `set_increment(f32)→u32`, `reset`,
  `sync_reset(frac)`, `u32` phase read.
- Migrate `Lfo` + `PolyLfo` (correctness; low-rate regression test).
- Migrate `Op` + `PolyOp` (perf + consistency; PM/feedback via `wrapping_add`
  offset, Sine via `lookup_sine_q32`, `phase_reset` via `sync_reset`).
- Regenerate + audition the audio goldens for the four modules.

**Out (deferred / other work):**

- `Oscillator` / `PolyOsc` migration — needs branchless fixed-point BLEP, ADR
  0080 Open question 1.
- Retiring the f32 accumulator — only once every consumer is Q32 (Open
  question 2).
- f64 increment for absolute tuning accuracy — orthogonal, not Q32's job.
- Hard-sync fixed-point rewrite — regressed in the bench; not pursued here.

## Tickets

- [x] [0970 — Q32 phase accumulator types in `patches-dsp`](../../tickets/closed/0970-q32-phase-accumulator-types.md)
- [x] [0971 — Migrate `Lfo` + `PolyLfo` to Q32 (low-rate correctness)](../../tickets/closed/0971-migrate-lfo-q32.md)
- [x] [0972 — Migrate `Op` + `PolyOp` to Q32 (perf + consistency)](../../tickets/closed/0972-migrate-op-q32.md)
- [x] [0973 — Regenerate + audition audio goldens for migrated modules](../../tickets/closed/0973-regenerate-goldens-q32.md)

## Dependency order

```text
0970 (accumulator types) ──> 0971 (LFO family) ──┐
                         └──> 0972 (Op family)  ──┴──> 0973 (goldens)
```

## Acceptance

- `patches-dsp` exposes `MonoPhaseAccumulatorQ32` / `PolyPhaseAccumulatorQ32`
  with determinism tests and a low-rate no-absorption test.
- `Lfo` / `PolyLfo` advance monotonically at 0.001–0.01 Hz (no stall); existing
  LFO behaviour tests pass (tolerances retuned where the table/representation
  shifts values).
- `Op` / `PolyOp` use the Q32 accumulator; the `osc_fixedpoint_bench` PolyOp
  delta is reproduced (Q32 materially below f32); existing Op tests pass.
- Audio goldens for the four modules regenerated after audition; the changed
  feedback patches (if any) noted in 0973.
- `just commit` green for touched crates; `cargo clippy` clean.

## Open questions

1. **Mono+poly per ticket vs split.** LFO mono/poly and Op mono/poly are bundled
   one ticket per family (identical change within a family). Split only if a
   family's migration turns out non-uniform.
2. **Bench as a gate.** Whether to wire an `osc_fixedpoint_bench` assertion into
   CI (like the 0958 ASM gate) or keep it a manual reference. Default: manual
   reference; these paths are not the do-or-die SIMD case.
