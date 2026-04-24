---
id: "E112"
title: CoefRamp primitive for filter-kernel coefficient smoothing
created: 2026-04-23
tickets: ["0653", "0654", "0655", "0656", "0657"]
adrs: []
---

## Goal

Factor the duplicated `begin_ramp` / per-sample coef-advance pattern shared
by `MonoBiquad`, `PolyBiquad`, `SvfKernel`, `PolySvfKernel`, `LadderKernel`,
`PolyLadderKernel`, `OtaLadderKernel`, `PolyOtaLadderKernel` into a single
`CoefRamp<K>` / `PolyCoefRamp<K, N>` primitive. Preserve the hot/cold field
split that the current kernels rely on for cache behaviour. No codegen or
perf regression on `tick_all` — this is dedup, not a performance play.

## Shape

```rust
pub struct CoefRamp<const K: usize> {
    pub active: [f32; K],
    pub delta:  [f32; K],
}
pub struct CoefTargets<const K: usize> {
    pub target: [f32; K],
}

pub struct PolyCoefRamp<const K: usize, const N: usize> {
    pub active: [[f32; N]; K],
    pub delta:  [[f32; N]; K],
}
pub struct PolyCoefTargets<const K: usize, const N: usize> {
    pub target: [[f32; N]; K],
}
```

`active + delta` lives in the kernel's hot region, `target` in the cold
region — two structs, not one, so the kernel keeps control of field order.

Methods: `begin_ramp(new_targets, &mut targets, interval_recip)`,
`begin_ramp_voice(i, new_targets, &mut targets, interval_recip)` on poly,
`advance()`, `set_static(values)`.

Kernel-specific extras (SVF `stability_clamp`, ladder `variant`) stay in
the kernel's `begin_ramp` wrapper that delegates to `PolyCoefRamp`.

## Scope

1. **Build primitive** — `patches-dsp::coef_ramp` with scalar + poly types,
   tests for snap-on-begin, endpoint drift, per-voice independence.
2. **Refactor `MonoBiquad` + `PolyBiquad`** — simplest kernel (no per-coef
   extras). One commit per kernel. `cargo test` + `cargo clippy` clean.
3. **Verify codegen** — disassemble `PolyBiquad::tick_all` before/after,
   confirm SIMD shape unchanged on both AVX2 (x86_64) and NEON (aarch64).
   Micro-bench if disassembly is ambiguous. Go / no-go gate.
4. **Refactor remaining kernels** — SVF, ladder, ota_ladder (mono + poly
   each). One commit per kernel.
5. **ADR** — capture the two-struct split rationale and the const-generic
   shape so future filter kernels follow the pattern.

## Non-goals

- Generalising beyond coefficient smoothing (no gain/mix/pan). The prior
  E111 attempt proved those sites don't exist in the current workspace.
- Changing the ramp *semantics*: span stays `interval_recip`-based (not
  pow2), snap-on-begin stays, no `remaining` counter.
- Changing kernel public APIs: call-site signatures may change slightly
  (5 positional args → `[f32; 5]`) but `begin_ramp` keeps its role.

## Tickets

- 0653 — Build `CoefRamp` / `PolyCoefRamp` in `patches-dsp`
- 0654 — Refactor `MonoBiquad` + `PolyBiquad` onto `CoefRamp`
- 0655 — Verify codegen / bench PolyBiquad; go/no-go gate
- 0656 — Refactor SVF, ladder, ota_ladder kernels
- 0657 — ADR for CoefRamp pattern
