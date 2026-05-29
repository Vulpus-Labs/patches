---
id: "0971"
title: Migrate Lfo + PolyLfo to Q32 phase (low-rate correctness)
priority: high
created: 2026-05-29
---

## Summary

Move `Lfo` and `PolyLfo` off their inline f32 phase to the Q32 accumulator
(0970). This fixes a real correctness defect: at the bottom of the LFO's own
rate clamp (`[0.001, 40.0]` Hz, `lfo.rs`), the f32 per-sample increment
(~`2.08e-8` at 0.001 Hz / 48 kHz) falls below the f32 ulp near `phase = 1.0`
(~`5.96e-8`) and is absorbed — the LFO stalls or steps unevenly near cycle end,
and slow ramps are warped by f32's non-uniform phase resolution. Q32 holds
uniform 32-bit resolution. No BLEP is involved (LFO is sub-audio; `lfo.rs`
already documents no BLEP on reset), so this is a phase-representation swap.

## Acceptance criteria

- [x] `Lfo` and `PolyLfo` use `MonoPhaseAccumulatorQ32` / `PolyPhaseAccumulatorQ32`
      instead of inline f32 phase.
- [x] Sine output reads `lookup_sine_q32`; naive saw / triangle / square read
      from the `u32` phase directly (no BLEP); per-voice `PolyLfo` spread
      multipliers still apply (computed as before, scaled into the `u32`
      increment).
- [x] `sync` / `sync_ms` phase reset preserved (plain phase set via the
      accumulator); `rate_cv` offset path preserved; the `[0.001, 40.0]` Hz
      clamp still applies before the increment is computed.
- [x] **Low-rate regression test:** at 0.001 Hz and 0.01 Hz the phase advances
      every sample (strictly increasing until wrap) and a full cycle completes
      in the expected sample count — the f32 stall does not occur.
- [x] Existing `Lfo` / `PolyLfo` behaviour tests pass; tolerances retuned only
      where the table/representation legitimately shifts values (note any in the
      PR).
- [x] `just commit -p patches-modules` green; `cargo clippy` clean.

## Notes

- ADR 0080 §2, Epic E159. Depends on 0970. Goldens regenerated in 0973.
- This is the correctness ticket of the epic; the win manifests at < ~0.1 Hz.
  Above ~1 Hz f32 was already fine — do not expect audible change there.
- `PolyLfo` spread: keep the per-voice rate distribution; only the accumulator
  representation changes.
