---
id: "0937"
title: SoA LFO phases in FdnReverb for vectorised sine + offset
priority: medium
created: 2026-05-19
---

## Summary

After 0936 brought the absorption stage down to ~3 ns (SIMD `BiquadN<8>`),
the next hottest sub-stage in `process_sample` was the LFO sine: line 179
of the old kernel — `fast_sine(self.lfo_phases[i].phase)` — at ~8.3 ns of
the ~40 ns kernel, plus ~3.2 ns of offset compute on line 182. The
`[MonoPhaseAccumulator; 8]` AoS layout blocked autovectorisation because
each iteration loaded one phase, called `fast_sine`, called `.advance()`,
then computed the offset, all serialised per line.

Same trick as 0936: turn into SoA. Replace the AoS array with
`lfo_phase: [f32; 8]` plus a shared `lfo_inc: f32` (all eight LFOs use
the same per-character rate), then express the sine, advance, and offset
compute as three independent `for i in 0..N` loops that LLVM
autovectorises.

## Acceptance criteria

- [x] `FdnReverbKernel` stores `lfo_phase: [f32; 8]` and `lfo_inc: f32`
      instead of `[MonoPhaseAccumulator; 8]`.
- [x] Per-sample LFO sine inlines the Bhaskara-with-Moser polynomial
      across the SoA array.
- [x] Phase advance uses branchless wrap (single conditional subtract).
- [x] Offset compute is its own SoA pass.
- [x] FDN tests pass; clippy clean.
- [x] Kernel bench shows measurable mean-ns/tick drop, 5-run stable.

## Outcome

Kernel-direct bench, 5 runs of 10M samples:

| variant                        | mean ns | p50   |
| ------------------------------ | ------- | ----- |
| original                       | 50.5    | 50    |
| + SIMD biquad (0936)           | 40.0    | 37–38 |
| + SoA LFO + offset (this)      | 36.5    | 36    |

**~9% additional speedup. Cumulative kernel: 50 → 36.5 ns ≈ 27%.**

Per-line samply confirms the cost moved:

| stage        | pre-0936 | post-0936 | post-0937 |
| ------------ | -------- | --------- | --------- |
| biquad ticks | ~17ns    | ~3ns      | ~3ns      |
| LFO sine     | ~8ns     | ~8ns      | ~3.3ns    |
| delay reads  | ~10ns    | ~10ns     | ~7.7ns    |

Remaining heaviest items are the **8 scattered delay-line reads (~7.7 ns)**
and **8 scattered delay-line writes (~4.7 ns)**. Each line owns its own
~38 KB buffer with a different LFO-driven offset, so there's no SoA
payoff available — these are inherently scalar memory accesses. We're at
sensible diminishing returns without restructuring delay storage or
accepting sound-altering changes (e.g. fewer lines).

## Notes

- All 8 LFOs share the same per-sample increment (per-character constant
  divided by sample rate), so a single `f32` suffices for `lfo_inc`.
  The previous AoS layout stored 8 redundant copies of it.
- Removed the now-unused `phase_accumulator` import; `MonoPhaseAccumulator`
  remains in patches-dsp for other consumers.
