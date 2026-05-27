---
id: E153
title: Tracker step event grammar (ADR 0077)
status: closed
created: 2026-05-20
---

## Goal

Implement [ADR 0077](../../adr/0077-tracker-step-event-grammar.md): a
unified step-event grammar that

- renames the tie token `~` → `_` (drops LSP/cable-endpoint clash);
- adds locally-readable cell shapes for "change pitch without
  retrigger" (`/value`) and multi-tick slides (`value>`, `>_`,
  `>value`);
- gives the row-build pass a single `StepEffect` enum as output,
  collapsing PatternPlayer's per-flag if-else cascade into a typed
  dispatch;
- generalises E152's roll-spread sub-event scheduling to a per-tick
  swung-aware schedule that also drives multi-tick slides.

Each ticket lands a tier of the change, with the working tree
playable at every boundary.

## Why all five tickets, not one

The change touches grammar, parser, AST, expansion, row-build,
runtime, LSP, manual, and goldens. A single PR would be unreviewable
and the audio behaviour shift around the swing-respecting schedule
would land in the same commit as the grammar rename — making
bisection painful when something breaks. Splitting along the data-
flow seams keeps each ticket testable in isolation and lets the
working tree keep parsing existing patches at every step until the
final rename is dropped in.

## Tickets

- [0943 — Introduce `StepEffect` row-build pass alongside the
  existing annotation](../tickets/open/0943-step-effect-row-build.md)
- [0944 — Pattern player consumes `StepEffect` (no behaviour
  change yet)](../tickets/open/0944-pattern-player-step-effect.md)
- [0945 — Per-channel sub-event schedule; respect per-tick swung
  durations](../tickets/open/0945-sub-event-schedule.md)
- [0946 — Grammar + parser: `_`, `/value`, `value>`, `>_`,
  `>value`](../tickets/closed/0946-step-grammar-extensions.md)
- [0948 — Abolish `slide()` macro; `:cv2` on multi-cell
  slides](../tickets/closed/0948-abolish-slide-macro-cv2-on-slides.md)
- [0947 — LSP hover + corpus + manual + goldens for the new
  shapes](../tickets/open/0947-step-grammar-lsp-and-docs.md)

## Acceptance

- Patches using the old `~` tie syntax are migrated in tree (one
  search-and-replace pass; documented in 0946's migration notes).
- `slide(n, start, end)` macro continues to lower correctly, now
  via `StepEffect` instead of direct AST shapes.
- All E152 tests still pass for non-swung patterns; swung
  `value*N _` patterns may shift by sub-sample amounts (goldens
  regenerated explicitly in 0945).
- `/value` is the canonical "change pitch without retrigger" form
  and replaces every author-written `slide(2, …)` invocation in
  the in-tree fixtures where the intent was instant change.
- `apply_step` dispatches on `StepEffect`, not on
  `(trigger, gate, cv1_end, repeat, absorbed_by_roll)`. The flag
  fields disappear from `TrackerStep` (replaced by an effect
  field).
- `just push` green at each ticket's boundary.

## Notes

- The ordering is deliberate: 0943–0944 introduce the new effect
  pipeline behind the existing surface grammar (no audible change
  until 0945's swing fix). 0945 lands the sub-event schedule and
  swing fix on top, with explicit golden regeneration for swung
  patterns. 0946 changes the surface grammar; existing patches
  break at this step and are migrated in the same PR. 0947 is the
  LSP + docs polish layer.
- Memory notes worth saving as we go: the **unified close rule**
  (bare `value` always = fresh trigger; lead-in shape is prior-cell
  business), the **continuation absorption asymmetry** (`*N`, `>`,
  `>_` all absorb `_`; the modifier that was open decides; bare
  `value` after them is *fresh trigger plus implicit slide close*),
  and the **`value>value` ≡ `value> /value`** sugar equivalence.
- The `slide(n, …)` macro keeps working but is no longer the
  primary form for multi-tick ramps; manual examples lead with the
  in-row cell forms.
