---
id: "0935"
title: patches-dsp — `fast_tan_small` approximation for bounded-range tan
priority: medium
created: 2026-05-19
---

## Summary

Add a small-angle `tan` approximation to `patches_dsp::approximate` for
filter kernels whose `g = tan(π·fc/sr)` coefficient is range-bounded
(e.g. the TPT SVF used by the bridged-T resonator family in
`patches-drums`, which clamps `g ≤ 0.4`). For the angle range
`[0, atan(0.4)] ≈ [0, 0.3805]` rad the function is smooth and
monotonic — a 5th-order Horner polynomial covers it to <1 cent of
pitch error at ~10× the throughput of `f32::tan`.

Motivates the per-sample SVF coefficient recompute that any
sample-rate-FM voice (Kick2, Tom2 self-FM via `lp_prev`; potentially
future per-partial FM uses of ModalBank) cannot avoid by ramping
alone.

## Proposed primitive

```rust
const G_MAX: f32 = 0.4;
const ANGLE_MAX: f32 = 0.380_506_4; // atan(G_MAX)

/// `tan(angle)` for `angle ∈ [0, atan(g_max)]`, output clamped to
/// `g_max`. 5th-order Taylor in Horner form. Caller must keep the
/// angle non-negative (the underlying SVFs already do).
#[inline]
pub fn fast_tan_small(angle: f32, g_max: f32) -> f32 {
    let x = angle.min(ANGLE_MAX);
    let x2 = x * x;
    let y = x * (1.0 + x2 * (1.0 / 3.0 + x2 * (2.0 / 15.0)));
    y.min(g_max)
}
```

3 mul + 2 add + 1 min (or 3 FMA on platforms with FMA). Versus libm
`tan` ≈ 30–50 cycles.

## Accuracy on `[0, 0.3805]`

| order | abs error @ 0.38 | rel error | pitch error |
|-------|------------------|-----------|-------------|
| 5th (proposed) | ~1.3e-4 | ~3.4e-4 | ~0.6 cents |
| 7th (`+ 17x⁷/315`) | ~5e-6 | ~1.2e-5 | ~0.02 cents |

5th-order is sufficient for any drum-voice FM use. Promote to 7th
only if a future caller surfaces an audible pitch artifact (none
known).

## Acceptance criteria

- [ ] `fast_tan_small(angle, g_max)` added to
      `patches_dsp::approximate` next to `fast_tanh`.
- [ ] Unit test: max abs error on a 1024-point sweep of
      `[0, atan(0.4)]` is below 2e-4.
- [ ] Unit test: monotonic over the sweep.
- [ ] Unit test: input `> atan(g_max)` returns exactly `g_max`
      (saturation).
- [ ] Doc-comment names the valid range and notes the post-clamp
      contract.
- [ ] Single consumer (`patches-drums::primitives::tpt_svf`) wired
      up in a follow-up ticket; merging that ticket is *not* a
      blocker for landing the primitive.

## Notes

**Per-crate convention** (per `tpt_svf` module docs) is "second
consumer gates promotion to patches-dsp". Only TPT SVF needs this
today. Two ways to land:

1. Add to `patches_dsp` now; document the single-consumer exception
   in the doc-comment ("kept here to keep `patches-drums` from
   carrying a private polynomial; second consumer expected when
   Kick2/Tom2 adopt CoefRamp"). This ticket.
2. Land crate-local in `patches-drums::primitives::approximate` for
   now; promote when a second consumer appears. Slower but matches
   existing policy.

Recommend (1) on the grounds that the primitive is a stable, tiny,
well-known polynomial whose API surface won't churn — the second-
consumer gate is most useful for primitives whose shape is still
being negotiated. Decide at review time.

**Related work**: this primitive is *complementary* to the
CoefRamp-for-TPT-SVF effort (separate ticket / ADR — see review
notes against the new `cymbal2` / `hihat2` / `xor_cymbal` / `xor_hihat`
modules). Ramping wins for shimmer-rate / param-rate modulation;
`fast_tan_small` wins for self-FM where ramping would alter the
audible character.

**Related primitive cleanup** (out of scope, flag for follow-up):
`TptSvf` divides `PI * fc / sample_rate` per sample. Caching
`pi_over_sr` would save one fdiv/tick — mirrors the `sr_recip`
pattern in `XorPairTone`.
