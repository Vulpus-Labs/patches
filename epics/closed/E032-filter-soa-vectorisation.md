---
id: "E032"
title: Filter SoA layout for SIMD vectorisation
status: closed
priority: medium
created: 2026-03-23
tickets:
  - "0182"
  - "0183"
---

## Summary

Profiling shows the polyphonic filter inner loops (`PolyBiquad`,
`PolySvfKernel`) are bottlenecked by the per-voice data-dependency chain
(2–3 levels deep) running serially across 16 voices. The current
Array-of-Structs (AoS) layout packs each voice's state into a 48-byte
cache line — good for single-voice access but opaque to SIMD vectorisation,
because s1/s2 values are stride-48 apart, preventing a packed SIMD load.

Reshaping to Structure-of-Arrays (SoA) makes each field contiguous across
all 16 voices (`s1: [f32; 16]`, `s2: [f32; 16]`, etc.) so LLVM can issue
a single 256-bit AVX2 load to read all 16 s1 values at once. Replacing
the per-voice `tick_voice(i, x)` call with a whole-frame
`tick_all(x: &[f32;16]) -> [f32;16]` call lets LLVM see all 16 independent
computations at once and auto-vectorise each step.

## Before (filter_bench, release, Apple Silicon)

```text
PolyLowpass (no-sat) :   24.1 ns/sample
PolyLowpass (saturate):   22.2 ns/sample
PolySvf              :   15.2 ns/sample
```

## After (filter_bench, release, Apple Silicon)

```text
PolyLowpass (no-sat) :    7.3 ns/sample   (was 24.1 → 3.3× faster)
PolyLowpass (saturate):   15.4 ns/sample  (was 22.2 → 1.4× faster)
PolySvf              :    7.4 ns/sample   (was 15.2 → 2.1× faster)
```

The no-saturation biquad path achieved 3.3×: all three recurrence steps
become independent per-element loops that LLVM vectorises with NEON
`fmul.4s`/`fadd.4s` (4 × f32 per instruction, 4 passes over 16 voices).
The saturate path achieved only ~1.4× because `fast_tanh` is a rational
polynomial with a division at its core. LLVM *does* emit `fdiv.4s` (verified
in assembly), so the path is vectorised — but `fdiv.4s` has ~3-4× lower
throughput than `fmul.4s` on M-series, keeping the saturate path at roughly
2× the cost of the linear path regardless of SoA layout. SVF achieved 2.1×
from fully vectorised `from_fn` steps across all three recurrence variables.

## Tickets

- [T-0182](../tickets/closed/0182-poly-biquad-soa-tick-all.md) —
  SoA layout for `PolyBiquad`; add `tick_all`; update `poly_filter.rs`
- [T-0183](../tickets/closed/0183-poly-svf-kernel-soa-tick-all.md) —
  SoA layout for `PolySvfKernel`; add `tick_all`; update `poly_svf.rs`

## Design notes

See the analysis discussion in the session that produced this epic for
the full SoA trade-off write-up (AoS vs SoA cache behaviour, saturation
complication, `begin_ramp_voice` interface preservation).

Key decisions:

- Remove `VoiceFilter` struct; hoist each field to `[f32; 16]` in
  `PolyBiquad`.
- Remove `VoiceSvf` struct; hoist fields to `[f32; 16]` in
  `PolySvfKernel`.
- `tick_voice(i, x, saturate, ramp)` → `tick_all(x, saturate, ramp)`
  returning `[f32; 16]`.
- `begin_ramp_voice(i, ...)` interface unchanged — still writes by index
  into SoA arrays.
- `set_static` / `new_static` / `has_cv` interfaces unchanged.
- Inner loops split into independent per-step loops so LLVM can vectorise
  each step.
- `saturate` path kept as a separate monomorphised branch (loop-invariant
  bool).
