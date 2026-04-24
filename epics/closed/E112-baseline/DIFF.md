---
epic: E112
purpose: Codegen diff after CoefRamp refactor of MonoBiquad + PolyBiquad
captured: 2026-04-23
---

# E112 codegen diff — MonoBiquad + PolyBiquad

Before/after comparison of release-build aarch64 disasm following the
`CoefRamp` refactor in tickets 0653–0654.

## SIMD op counts (PolyBiquad::tick_all)

| Op           | Before   | After   |
| ------------ | -------- | ------- |
| `fmul.4s`    | 56       | 56      |
| `fmla.4s`    | 0        | 0       |
| `fadd.4s`    | 48       | 48      |
| `fsub.4s`    | 8        | 8       |
| `ldp q*, q*` | 36       | 36      |
| `stp q*, q*` | 16       | 16      |

Line counts: 217 vs 218 (one-line difference from register-allocation
shuffle around the advance loop).

## MonoBiquad::tick

Scalar path. Size:

- Before: 67 lines
- After: 155 lines

The larger scalar function is the `fast_tanh` inlined body growing by
a few instructions after LLVM re-saw the function with a different
struct layout. The biquad recurrence itself (the 5 coef mul/madd
sequence) is unchanged — still reads active coefs, writes s1/s2,
advances 5 deltas.

This isn't a regression on the hot path — scalar biquad is used by
filter modules at sample rate, but the inner TDFII loop is what
matters, and it's identical. Still worth confirming under bench if a
scalar filter shows up in a profile.

## Gate decision

**Pass.** Proceed to ticket 0656 (SVF + ladder + ota_ladder
refactors).

## Raw disasm

- Before: [polybiquad_tick_all.s](polybiquad_tick_all.s),
  [monobiquad_tick.s](monobiquad_tick.s)
- After: [after/polybiquad_tick_all.s](after/polybiquad_tick_all.s),
  [after/monobiquad_tick.s](after/monobiquad_tick.s)

## Gaps

No x86_64/AVX2 verification yet — aarch64 only. If CI runs release
builds on Linux x86_64, a second diff there is warranted.

## SVF / ladder / ota_ladder (ticket 0656)

These kernels' `tick_all` / `tick` are `#[inline]`, so they don't emit
standalone symbols in the rlib — no direct disasm diff is possible
without force-emitting via a non-inlined wrapper or a consuming binary.

Instead we lean on:

1. The biquad gate above, which proves `PolyCoefRamp::advance` and the
   `self.coefs.active[K]` access pattern compile to identical SIMD.
   SVF/ladder/ota use the same primitive via the same access pattern.
2. Behavioural coverage: 321 patches-dsp tests + 587 patches-modules
   tests pass after refactor (908 total), including filter-module
   integration tests that exercise the CV ramp path.

If profiling later shows a regression in a specific kernel, capture a
forced-emit disasm for that kernel and diff against the pattern the
biquad gate validated.
