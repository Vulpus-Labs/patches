---
id: "0653"
title: Build CoefRamp / PolyCoefRamp in patches-dsp
priority: medium
created: 2026-04-23
epic: E112
---

## Summary

Add `patches-dsp::coef_ramp` with scalar + poly structs that capture the
snap-on-begin / store-target / compute-delta / per-sample-advance pattern
duplicated across the filter kernels. Two structs per arity (hot
`active + delta`, cold `target`) so kernels keep control of cache layout.

## Acceptance criteria

- [ ] `CoefRamp<const K: usize>`: fields `active: [f32; K]`, `delta: [f32; K]`.
- [ ] `CoefTargets<const K: usize>`: field `target: [f32; K]`.
- [ ] `PolyCoefRamp<const K: usize, const N: usize>`: SoA per coef,
      `active: [[f32; N]; K]`, `delta: [[f32; N]; K]`.
- [ ] `PolyCoefTargets<const K: usize, const N: usize>`: `target: [[f32; N]; K]`.
- [ ] Methods:
      - `CoefRamp::new(values: [f32; K])` and `new_static` equivalent
      - `CoefRamp::set_static(&mut self, values: [f32; K])` — zeroes delta
      - `CoefRamp::begin_ramp(&mut self, new_targets: [f32; K], targets: &mut CoefTargets<K>, interval_recip: f32)`
      - `CoefRamp::advance(&mut self)` — `active[k] += delta[k]`
      - Poly equivalents; poly also has `begin_ramp_voice(i, new_targets, targets, interval_recip)`.
- [ ] Unit tests: snap-on-begin swaps prev-target into active; delta
      magnitude matches `(target - active) * interval_recip`; `advance` run
      `span` times lands on target within rounding; `set_static` zeroes
      delta; per-voice independence on poly.
- [ ] No allocations. No new deps.
- [ ] `cargo clippy` clean (no new warnings).

## Notes

Also capture baseline disasm of `MonoBiquad::tick` and
`PolyBiquad::tick_all` against current (unrefactored) code as part of
this ticket — saved under `epics/open/E112-baseline/` — so ticket
0655's comparison is a real diff, not reconstructed from memory.

Methods must be `#[inline]` — these are per-sample hot-path structures.
For poly `advance`, the inner `for i in 0..N` loop over the fixed-size
`[f32; N]` is what autovec needs; the outer `for k in 0..K` unrolls
(K is const, small: 2, 3, 5).

Do not add a `remaining` counter or snap-to-exact-target logic — that's
the `Ramp` design the prior epic proved had no callers. Here drift is
handled by snapping `active ← previous target` at the *start* of the
next `begin_ramp`.
