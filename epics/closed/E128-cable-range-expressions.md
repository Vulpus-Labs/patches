---
id: "E128"
title: Cable range expressions (uni / bi) with units
created: 2026-04-30
tickets: ["0765", "0766", "0767", "0768", "0769", "0770"]
adrs: ["0062"]
---

## Goal

Implement [ADR 0062](../../adr/0062-cable-range-expressions.md):
add `uni(lo, hi)` and `bi(lo, hi)` range forms to the cable arrow
syntax, mapping `[0,1]` and `[-1,1]` source ranges onto destination
ranges with hard clipping. Endpoints share the existing `scale_val`
forms (numbers, unit-suffixed literals, note literals, `<param>`
refs); pitch-family endpoints (notes + Hz) unify to v/oct so
`bi(C1, 2kHz)` is valid.

The runtime mechanism is per-input-port `(scale, offset, clip)`
applied at read time. Pure-scalar cables retain the existing fast
path (`offset = 0`, `clip = None`).

## Scope

This was originally tracked as ticket 0764. Promoted to epic because
the implementation crosses grammar, AST, parser, expander, flat
schema, interpreter, port runtime, builder, LSP, and docs — too many
seams for one merge.

The break-out below stages the work so each ticket is independently
shippable: grammar + AST land first as additive surface (parses but
no runtime semantics yet); the runtime triple lands second on the
scalar fast path with no behavioural change; the composition algebra
and clip math land third; lowering and the param-ref recompute path
land fourth; LSP and docs close it out.

## Acceptance

- ADR 0062 implemented end-to-end across DSL, expander, interpreter,
  core port runtime, engine builder, and LSP.
- Existing pure-scalar cables unchanged at runtime (verified by a
  microbench on `MonoInput::read`).
- `bi(C1, 2kHz)` lowers to v/oct on both endpoints; `bi(440Hz,
  -12dB)` is rejected with a clear unit-family diagnostic.
- `<param>` endpoints recompute `(scale, offset, clip)` on parameter
  update via the existing port-update path.
- LSP hover on a range-mapped cable shows resolved endpoints.
- `docs/src/dsl-reference.md` cable-scale section updated.
- `cargo test` and `cargo clippy` pass on the inner-loop subset and
  full workspace.
