---
id: "0765"
title: Cable range grammar and AST (uni / bi parsing)
priority: medium
created: 2026-04-30
epics: ["E128"]
adrs: ["0062"]
---

## Summary

Add `uni(lo, hi)` and `bi(lo, hi)` forms to the cable-scale arrow,
parsing only — no semantic change downstream. Pure-scalar cables
remain identical end-to-end. Range cables parse into a new AST
variant and are ignored by the expander/interpreter for now (they
should error cleanly with "not yet implemented" at expand time, so
the surface is reachable for testing without breaking builds).

## Acceptance criteria

- [x] `patches-dsl/src/grammar.pest`: `scale_val` extended so the
      arrow accepts either today's single value or
      `("uni" | "bi") ~ "(" ~ scale_endpoint ~ "," ~ scale_endpoint ~ ")"`,
      where `scale_endpoint` is the previous `scale_val` body
      (param_ref | float_unit | scale_num | note_lit). Existing
      single-value cables parse identically.
- [x] `Arrow.scale: Option<Scalar>` becomes
      `Arrow.scale: Option<ScaleSpec>` where
      `ScaleSpec { Scalar(Scalar), Range { kind: RangeKind, lo: Scalar, hi: Scalar } }`
      and `RangeKind { Uni, Bi }`. Span on `ScaleSpec`.
- [x] Parser builds the new variant; existing `build_scale_val`
      becomes `build_scale_endpoint` and is reused for both endpoints.
- [x] Expander's `eval_scale` keeps the existing scalar path; range
      variant returns `ExpandError(InvalidCableScale, "not yet
      implemented in this build")`. Tracking comment points at this
      ticket and the next one.
- [x] Parser-level tests cover: `-[uni(0, 1)]->`, `-[bi(C1, 2kHz)]->`,
      `-[uni(<lo>, <hi>)]->`, and a syntax error on `-[uni(0)]->`.
- [x] `cargo test -p patches-dsl` and `cargo clippy` pass.

## Notes

Reference: [ADR 0062](../../adr/0062-cable-range-expressions.md) and
epic [E128](../../epics/open/E128-cable-range-expressions.md).

This ticket is deliberately additive: the goal is to make range
syntax a first-class parse target so subsequent tickets can fill in
semantics without grammar churn. Note literals already exist as
`scale_val` children via `float_unit` parsing? — verify: today's
`scale_val = ${ param_ref | float_unit | scale_num }` does not
include `note_lit`. Add `note_lit` to the endpoint rule so
`bi(C1, 2kHz)` parses (semantic v/oct unification lives in 0766).
