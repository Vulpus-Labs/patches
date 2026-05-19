---
id: "0935"
title: FdnReverb per-sample perf — drop Thiran, gate biquad ramp, fold INV_SQRT8
priority: medium
created: 2026-05-19
---

## Summary

`FdnReverb` is the top per-sample CPU consumer in the drum patch. Three
sonically-transparent changes in `FdnReverbKernel::process_sample` should
cut per-sample cost substantially.

## Acceptance criteria

- [x] FDN delay-line reads use linear interpolation in place of first-order
      Thiran all-pass — no division, no per-line `y_prev` state, no frac clamp.
- [x] `MonoBiquad` gains a `tick_static` (or equivalent) entry point that
      skips per-sample `coefs.advance()`; FDN kernel dispatches to it when
      `cv_connected == false`.
- [x] Output sum (`OUT_L`/`OUT_R`) is rewritten as sign-pattern adds with
      `INV_SQRT8` folded into the `wet` gain.
- [x] Existing FDN tests still pass: impulse decays, DC bounded, stereo
      decorrelation, early reflections, bounded energy across all characters.
- [x] Audio goldens that include FDN may shift by tiny epsilons; audition
      and regenerate if they sound identical (treat like the fusion
      regeneration policy in project memory).
- [x] `cargo clippy` clean for touched crates; `just inner -p patches-modules
      -p patches-dsp` green.

## Notes

### Why these three

Per-sample budget (8 lines, 48 kHz) breakdown identified:

1. **8 divisions/sample** in `ThiranInterp::read` (`(1-frac)/(1+frac)`).
   Thiran's flat group delay matters for chorus/vibrato where the
   modulated delay *is* the signal. In a reverb, the LFO depth here is
   0.3–2.0 ms only to decorrelate modes; linear interp is
   sonically indistinguishable. Removing divisions is the largest single win.
2. **40 wasted adds/sample** in `MonoBiquad::tick` calling `coefs.advance()`
   even when deltas are zero (no CV connected, the common standalone case).
   `PolyBiquad` already has the `has_cv` short-circuit; `MonoBiquad` lacks it.
3. **16 muls in the output sum** with `OUT_L`/`OUT_R` patterns that are
   ±`INV_SQRT8`. Replace with 14 adds and a single `* (INV_SQRT8 * wet)`.

### Out of scope (other ideas considered)

- Triangle LFO instead of `fast_sine` — subtle sound change, defer.
- Loop fusion of `raw[]` / `damp[]` / output sum — possible but compiler
  often already eliminates the stack temporaries; revisit if profiling
  shows it still matters after the three above land.
- Global FTZ/DAZ dependency for dropping `flush_denormal` in
  `MonoBiquad::tick` — global biquad change, not FDN-specific.

### Validation

- `cargo bench` / patches-profiling: time `process_sample` on a fixed
  sequence before and after, report ratio.
- Audition the drum patch (the motivating workload).
- Regenerate audio goldens that include FDN if and only if they sound
  identical to the pre-patch reference.

### Outcome

Microbench (`patches-profiling/src/bin/fdn_reverb_bench.rs`, 48 kHz, 2M
ticks, sine input):

|      | mean ns/tick | p50 | p90 | p99 |
| ---- | ------------ | --- | --- | --- |
| pre  | 132.5        | 123 | 155 | 294 |
| post | 133.9        | 123 | 160 | 302 |

No measurable improvement. The arithmetic-savings hypothesis was wrong.
Two plausible reasons:

1. **Memory-bound, not compute-bound.** Each line owns its own ~38 KB
   delay buffer (200 ms @ 48 kHz). Scattered L2-resident reads dominate
   the cycle budget; saving a division or five adds in the shadow of a
   load-use stall is invisible.
2. **LLVM was already doing the work.** Constant propagation through
   inlined `tick` + zero deltas + ±INV_SQRT8 patterns very likely
   collapsed to the same machine code we'd hoped to hand-craft.

Changes kept because they're sonically identical, slightly simpler, and
the new `tick_static` is useful for other modules.

Next-step ideas (not in this ticket):

- Profile with `samply` / `cargo flamegraph` on the drum patch to confirm
  the memory-bound hypothesis.
- Try `LINES = 4` as an A/B — halves arithmetic and memory pressure;
  audible thinning of the late field that may or may not matter on a
  drum bus.
- Block processing for the kernel — needs DSL/engine support to feed
  N samples per call; large rework.
