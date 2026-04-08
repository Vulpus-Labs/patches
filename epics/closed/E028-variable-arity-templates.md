---
id: "E028"
title: Variable-arity template ports and parameters
status: closed
priority: high
created: 2026-03-20
tickets:
  - "0164"
  - "0165"
  - "0166"
  - "0167"
---

## Summary

Adds variable-arity port groups and parameter groups to templates. A template
may declare `in: audio[n]` (n ports named `audio`) and `level[n]: float`
(n float params), where `n` is a template parameter. In connections, `[*n]`
triggers arity expansion — the expander emits n concrete connections, one per
index. `[k]` (bare ident) addresses a single port at a computed index.

All expansion happens in Stage 2 (expander). `FlatPatch`, Stage 3, `ModuleGraph`,
and the audio engine are unchanged.

Depends on E027 (the `<param>` syntax changes must land first, as this epic
uses `<n>` in shape args and both share the `PortIndex` / `Scalar` AST types).

See ADR 0019 for the full design.

## Tickets

- [T-0164](../tickets/open/0164-arity-ast.md) — AST: `PortIndex` enum, arity on port declarations, group params in `ParamDecl`
- [T-0165](../tickets/open/0165-arity-parser.md) — Parser: `[n]`/`[*n]` index syntax, `name[n]` in port and param declarations
- [T-0166](../tickets/open/0166-arity-expander.md) — Expander: `[*n]` expansion, group param broadcast/array/per-index
- [T-0167](../tickets/open/0167-arity-tests.md) — Tests: expansion correctness, error cases, boundary rewiring with arity
