---
id: "0657"
title: ADR for CoefRamp pattern
priority: low
created: 2026-04-23
epic: E112
depends_on: ["0656"]
---

## Summary

Write an ADR capturing the `CoefRamp` / `PolyCoefRamp` design so future
filter kernels follow the same pattern without re-deriving the hot/cold
split or the snap-on-begin rationale.

## Acceptance criteria

- [ ] New `adr/NNNN-coef-ramp.md` covering:
      - The per-sample coefficient smoothing pattern (what duplicated
        and why factoring helped).
      - Why two structs (`CoefRamp` + `CoefTargets`) not one —
        preserving the hot/cold cache split.
      - Why no `remaining` counter / no exact-endpoint snap: drift is
        handled by snap-on-begin at the next update boundary.
      - How to handle per-kernel extras (SVF's `stability_clamp`,
        ladder's `variant`): wrapper methods on the kernel, not
        generalised into `CoefRamp`.
      - Codegen verification from ticket 0655 (disasm/bench summary).
- [ ] Link the ADR from `patches-dsp/src/coef_ramp.rs` doc comment.

## Notes

Short ADR — a page or two. The pattern is small, the trade-offs are
clear. Longer only if codegen turned up surprises.

If 0655 forced a revert, rewrite this ticket as a "why it didn't work"
postmortem ADR instead — that's arguably more valuable for future
attempts.
