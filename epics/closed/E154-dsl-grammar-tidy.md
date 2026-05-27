---
id: E154
title: DSL grammar tidy
status: closed
created: 2026-05-20
---

## Goal

The pest grammar in `patches-dsl/src/grammar.pest` has accumulated
small inconsistencies and one latent parse hazard during the
roll-up of recent surface work (host controls, stereo sugar, step
event grammar). This epic groups two narrow tidy-ups:

- Fix two latent parse bugs (`bool_lit` word-boundary, `step_valued`
  atomicity) before they bite.
- Consolidate duplicate numeric rules and the repeated
  word-boundary lookahead pattern into a shared form.

No surface-syntax change is intended. Every existing `.patches`
file must still parse identically, and the corpus driver must
still report tree-sitter / pest parity.

## Why two tickets, not one

The bugs are user-visible misparses; the consolidation is a
refactor with no behaviour change. Splitting keeps the bug-fix
commit small and bisectable, and lets the refactor land on a
clean foundation. The grammar comment + tap_component +
host_control_field tightening, plus the `value>value*N` grammar
rejection, are folded into the consolidation ticket since they
are all "narrow the grammar without surface change."

## Tickets

- 0949 — Grammar parse bugs: `bool_lit` word-boundary, `step_valued`
  atomicity.
- 0950 — Grammar consolidation: merge `step_*` numeric duplicates
  with `*_lit`, extract word-boundary helper, reject
  `value>value*N` at the grammar level, tighten
  `host_control_field`, inline `tap_component`, drop misleading
  "legacy form" comment.

## Acceptance

- Existing `.patches` files in `examples/`, `patches-dsl/tests/`,
  and `docs/` parse identically before and after.
- `patches-lsp/tests/syntax_corpus/` passes the parity driver.
- New negative-case fixtures lock in the bugs:
  - `true-foo` no longer matches as `bool_lit` plus dangling `-foo`.
  - `C4 : 0.5` (whitespace inside a `step_valued` cell) is rejected.
  - `C4>E4*3` (slide sugar + roll) is rejected at parse time, not
    by a defensive post-parse check.
- `just push` green at each ticket boundary.

## Out of scope

- ADR-codified surface (param/port arity wildcards, `port[<name>]`
  explicit-param-ref form) — kept as-is.
- `StepKind::StepTo { cv2 }` field — held for `/value:cv2`
  landing per ticket 0947 surface reservation.
- New surface syntax of any kind.
