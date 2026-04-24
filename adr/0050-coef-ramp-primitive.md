# ADR 0050 — CoefRamp primitive for filter-kernel coefficient smoothing

**Date:** 2026-04-23
**Status:** accepted

---

## Context

Before epic E112, eight filter kernels in `patches-dsp` each carried
their own copy of the same five-move pattern for per-sample coefficient
interpolation:

1. Store active coefficients (read every sample by the recurrence).
2. Store target coefficients (read only at update boundaries).
3. Store per-sample deltas.
4. On `begin_ramp`: snap active ← previous target (drift guard), store
   new target, compute `delta = (new_target - active) * interval_recip`.
5. On every `tick`: `active[k] += delta[k]`.

Kernels affected: [`MonoBiquad`](../patches-dsp/src/biquad/mod.rs),
[`PolyBiquad`](../patches-dsp/src/biquad/mod.rs),
[`SvfKernel`](../patches-dsp/src/svf/mod.rs),
[`PolySvfKernel`](../patches-dsp/src/svf/mod.rs),
[`LadderKernel`](../patches-dsp/src/ladder/mod.rs),
[`PolyLadderKernel`](../patches-dsp/src/ladder/mod.rs),
[`OtaLadderKernel`](../patches-dsp/src/ota_ladder/mod.rs),
[`PolyOtaLadderKernel`](../patches-dsp/src/ota_ladder/mod.rs).

K (coefficient count) varies: biquad K=5, SVF K=2, ladder / OTA K=3.
The pattern is mechanical and identical up to K. A bug fixed in one
(e.g. a missing drift snap) had to be replicated in seven others.

A prior attempt (epic E111) tried to generalise *all* smoothing
(filter coefs, gains, pans, mixes) behind one `Ramp<T>` primitive with
a `remaining` counter and exact endpoint snap. The survey found no
gain / pan / mix sites with the matching shape — the only real
duplication was filter coefficient smoothing. E111 was reverted.

## Decision

Introduce [`patches-dsp::coef_ramp`](../patches-dsp/src/coef_ramp.rs)
with four types parameterised on a const `K` (and `N` for poly):

```rust
pub struct CoefRamp<const K: usize> {
    pub active: [f32; K],
    pub delta: [f32; K],
}
pub struct CoefTargets<const K: usize> {
    pub target: [f32; K],
}
pub struct PolyCoefRamp<const K: usize, const N: usize> {
    pub active: [[f32; N]; K],
    pub delta: [[f32; N]; K],
}
pub struct PolyCoefTargets<const K: usize, const N: usize> {
    pub target: [[f32; N]; K],
}
```

Methods: `new(values)`, `set_static(values)`,
`begin_ramp(new_targets, &mut targets, interval_recip)`, `advance()`,
and `begin_ramp_voice(i, ...)` on the poly variants.

Each filter kernel holds a `CoefRamp` in its hot region and a
`CoefTargets` in its cold region. Per-kernel extras (SVF's
`stability_clamp`, ladder's `LadderVariant`, OTA's `OtaPoles`) remain
on the kernel, not in the primitive — wrapper methods build the
`[f32; K]` target array and delegate to `CoefRamp::begin_ramp`, or
(for SVF's clamp-on-snap) write into the `pub` fields directly.

## Why two structs, not one

The hot/cold split matters for cache behaviour: `tick_all` reads
`active` every sample but `target` only at update boundaries. Baseline
kernels placed target fields at the end of the struct for this reason.

A single `CoefRamp { active, delta, target }` would interleave the
cold `target` array in the middle of the hot region and pollute the
cache lines read per sample. Splitting into `CoefRamp` (hot) +
`CoefTargets` (cold) lets each kernel place them at the right points
in its own struct layout. The kernel keeps control; the primitive
doesn't impose a layout.

## Why no `remaining` counter or endpoint snap

Drift is handled by snapping `active ← previous target` at the
**start** of the next `begin_ramp` call, matching existing kernels.
The ramp advances continuously between update boundaries; the next
retarget re-bases off the stored (exact) target, not the drifted
active.

Adding a `remaining: u16` + "snap active := target when remaining
hits 0" would:

- Add a per-sample branch in the hot path.
- Require pow2-span retargeting (the E111 design), which doesn't
  match this system's `interval_recip`-based cadence.
- Solve a problem that snap-on-begin already solves.

## Codegen verification

The biquad refactor was used as the gate (see
[`epics/closed/E112-baseline/DIFF.md`](../epics/closed/E112-baseline/DIFF.md)).
On aarch64 (Apple Silicon, NEON), `PolyBiquad::tick_all` produces
identical SIMD op counts before and after the refactor:

| Op | Before | After |
| --- | --- | --- |
| `fmul.4s` | 56 | 56 |
| `fadd.4s` | 48 | 48 |
| `fsub.4s` | 8 | 8 |
| `ldp q*, q*` | 36 | 36 |
| `stp q*, q*` | 16 | 16 |

SVF / ladder / ota `tick_all` are `#[inline]`, so they don't emit
standalone symbols to disasm directly; they share the same
`PolyCoefRamp::advance` and `active[K]` access pattern that the biquad
gate validated. Not captured on x86_64 / AVX2 — noted as a gap.

## Consequences

**Positive:**

- Eight near-identical implementations collapsed to one generic
  primitive + eight thin wrappers.
- A future filter kernel (e.g. fifth-order EQ, comb, phaser
  all-pass) gets the snap-on-begin / delta-advance protocol for free
  by declaring `coefs: CoefRamp<K>` and `targets: CoefTargets<K>`.
- Bug fixes to the ramp protocol (e.g. if we later need to add
  subnormal flush at retarget) happen in one place.

**Negative:**

- One extra layer of naming: `self.coefs.active[B0]` instead of
  `self.b0`. Kernel tick bodies introduce local `let` bindings to
  restore readability.
- Kernels with clamp-on-snap invariants (SVF) bypass
  `CoefRamp::begin_ramp` and write the `pub` fields directly. This is
  deliberate — the primitive doesn't try to absorb kernel-specific
  invariants — but it means `pub active` / `pub delta` /
  `pub target` must stay public.
- Codegen equivalence depends on LLVM seeing through
  `[[f32; N]; K]` indexing to the same SIMD shape as named
  `[f32; N]` fields. Verified on aarch64; unverified on x86_64.

## Alternatives considered

- **Single struct with `target` inline.** Rejected: breaks hot/cold
  cache split.
- **Trait `Ramp<const K>` implemented per kernel.** Rejected: adds
  dynamic dispatch risk and virtualises what is fundamentally a data
  layout, not a behaviour.
- **Generic over `T: Float` (f32/f64/f64-simd).** Rejected: every
  caller is f32; adding num-traits or a custom float trait for zero
  callers is speculation.
- **Keep hand-rolled.** Rejected: the duplication was real and
  error-prone; the primitive is small (~150 LOC) and proven
  codegen-equivalent.

## References

- Epic: [`epics/open/E112-coef-ramp-primitive.md`](../epics/open/E112-coef-ramp-primitive.md)
- Codegen diff: [`epics/closed/E112-baseline/DIFF.md`](../epics/closed/E112-baseline/DIFF.md)
- Prior-art counter-example (reverted): the E111 `Ramp<T>` attempt,
  which showed that generalising *beyond* coefficient smoothing found
  no callers.
