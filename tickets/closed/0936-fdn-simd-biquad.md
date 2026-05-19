---
id: "0936"
title: SIMD biquad in FdnReverb absorption stage
priority: medium
created: 2026-05-19
---

## Summary

The samply profile of `FdnReverbKernel::process_sample` (50M samples,
kernel-direct) shows **34.5% of self-time in `MonoBiquad::tick`** —
eight scalar biquads, one per delay line, run in sequence. Replacing
them with one SoA biquad covering all eight voices lets LLVM
auto-vectorise the recurrence (NEON 4-lane f32, two passes per step)
and should knock ~10–15 ns off the ~50 ns/tick kernel cost (~20–30%
kernel speedup).

The pattern is already proven in `PolyBiquad` — a 16-voice SoA biquad
used by `poly_filter`. This ticket generalises it to arbitrary N and
applies it to the FDN.

## Acceptance criteria

- [x] `patches-dsp` exposes a generic `BiquadN<const N: usize>` with the
      same surface as the existing `PolyBiquad` (new_static, set_static,
      begin_ramp_voice, tick_all, has_cv).
- [x] `PolyBiquad` becomes `type PolyBiquad = BiquadN<16>` — existing
      `poly_filter` call sites and tests untouched.
- [x] `BiquadN` gains `set_static_voice(i, [b0,b1,b2,a1,a2])` so the FDN
      can install per-line static coefficients into one struct.
- [x] `FdnReverbKernel::absorption` becomes `BiquadN<8>`. Per-line
      coefficient install paths (`prepare`, `apply_static_absorption`,
      `recompute_absorption`) use the per-voice setters.
- [x] Per-sample kernel calls `absorption.tick_all(&raw)` instead of an
      eight-iteration scalar loop.
- [x] All `MonoBiquad`-flavour validation invariants in
      `patches-dsp/src/biquad/tests/` still pass for `PolyBiquad` (i.e.
      no behaviour change for the existing N=16 alias).
- [x] `FdnReverb` tests still pass: impulse decays, DC bounded, stereo
      decorrelation, early reflections, bounded energy.
- [x] Kernel-direct bench (`patches-profiling/src/bin/fdn_kernel_bench`)
      shows a measurable mean-ns/tick drop, three-run stable.
- [x] `cargo clippy` clean; `just inner -p patches-modules -p patches-dsp` green.

## Outcome

Kernel-direct bench, 5 runs of 10M samples each (mean ns/tick):

|                               | mean                         | p50   |
| ----------------------------- | ---------------------------- | ----- |
| pre (scalar `MonoBiquad` × 8) | 50.5                         | 50    |
| post (`BiquadN<8>::tick_all`) | 40.4, 39.5, 40.6, 39.0, 45.8 | 37–38 |

**~20% kernel speedup, repeatable.** First measured perf win in this
investigation.

Samply self-time confirms the cost moved as expected:

| stage                   | pre   | post      |
| ----------------------- | ----- | --------- |
| `process_sample` body   | 55.3% | **82.0%** |
| `MonoBiquad::tick` × 8  | 34.5% | —         |
| `BiquadN::tick_all` × 1 | —     | **7.8%**  |

Biquad stage absolute cost: **~17 ns → ~3 ns** (5×). Total kernel cost
50 ns → 40 ns. Delay reads + LFOs + hadamard + output sum now dominate
the remaining cost; further wins would need to attack those (SIMD the
delay reads, manual unroll, etc.).

## Notes

- The N=8 case lines up well with NEON (8 × f32 = 2× 128-bit ops). On AVX2
  it's one 256-bit op per step.
- Per-line coefficients differ (delay_ms varies), so the FDN cannot use
  the broadcast `new_static`/`set_static` paths. The `set_static_voice`
  addition is the minimal API delta.
- The `tick_all` body in `PolyBiquad` already structures the recurrence
  as four independent per-element loops — exactly the shape LLVM
  auto-vectorises. The generalisation to N is mechanical.
- The committed ticket 0935 changes (linear interp, `tick_static`,
  sign-only sum) had no measurable effect; profiling pointed here as the
  next real lever before any sound-changing trade-off (e.g. LINES=4).
