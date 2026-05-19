---
id: E152
title: Tracker tie-span repeats (roll *N across multiple ticks)
status: in-progress
created: 2026-05-19
---

## Goal

Today `x*3` fires three sub-triggers inside a single tick by dividing
`current_tick_duration_samples` by N
([`pattern_player/mod.rs:220-237`](../../patches-modules/src/tracker_core/pattern_player/mod.rs#L220-L237)).
There is no way to spell "triplet rolled across two ticks", which is a
common drum/percussion figure. This epic adds **tie-span repeats**:
when a `*N` step is followed by one or more tie cells (`~`), the N
sub-triggers are spread evenly across the combined tick span instead
of crammed into the anchor tick.

```text
x*3        // 3 triggers in 1 tick  (existing)
x*3 ~      // 3 triggers in 2 ticks (triplet over 2)
x*5 ~ ~    // 5 triggers in 3 ticks (quintuplet over 3)
```

## Why ties (not new syntax)

`|` was the user's first instinct but already means row continuation
([`grammar.pest:234`](../../patches-dsl/src/grammar.pest#L234)). `:` is
already cv2. Tie (`~`) already exists with the right intuition —
"this cell continues the previous" — and is the visually obvious choice
in a tracker grid. No grammar change required.

## Semantic disambiguation

A tie's meaning depends on the anchor step:

| Anchor                | Tie behaviour                                      |
|-----------------------|----------------------------------------------------|
| Plain step (no `*N`)  | **Sustain** (current): gate stays high, no trigger |
| `*N` step             | **Spread**: extends the roll across this tick      |

The sustain semantics are preserved unchanged for the common case. The
spread interpretation kicks in only when the anchor's `repeat > 1`.

## Scope

- Parser/AST: no grammar change. `Step.repeat: u8` already in place.
- Interpreter (row build): annotate each `*N` anchor with a derived
  `repeat_span: u8` = 1 + number of consecutive following ties.
  Annotate the consumed tie cells with a flag marking them as
  "absorbed by prior roll" so the pattern player does not re-evaluate
  them as plain ties.
- Pattern player: when applying a `*N` anchor with `repeat_span > 1`,
  compute interval as `(tick_duration_samples * span) / N` and let the
  existing sample-counter schedule run past the next tick edge.
  Suppress trigger/gate effects from absorbed tie cells.
- Sequencer: no change to tick emission. The roll-state survives tick
  rises because it lives on the pattern player.
- Docs (manual + DSL hover) and golden audio tests.

## Out of scope

- **Explicit ratio syntax** like `x*3/2`. Maybe later; tie-spread
  covers the same expressive ground and reads better in a pattern
  grid. Revisit only if real authoring pain shows up.
- **Swing within a span**. If the span crosses a swung beat boundary,
  the v1 implementation uses the anchor tick's duration for the whole
  span (uniform interval). Real per-tick interval recomputation is a
  follow-up if the audible glitch matters in practice.
- **Cross-pattern-loop spans**. If a `*N ~` figure straddles the loop
  point, overflow triggers are dropped (truncate, don't wrap). The row
  owns its own triggers.
- **Cross-bank spans**. Same as loop boundary — truncate at bank edge.
- **Sustain-tie + roll-tie mix in one chain**. After absorbed-by-roll
  ties, the next non-absorbed cell is a normal step (rest, tie,
  trigger). The roll cannot itself "tie out" past its span — author
  writes a follow-on tie cell against a new anchor if needed.

## Tickets

- [0939 — Annotate Step rows with repeat_span derived from following ties](../tickets/open/0939-tracker-repeat-span-row-build.md)
- [0940 — Pattern player: spread *N triggers across span samples](../tickets/open/0940-tracker-pattern-player-tie-span.md)
- [0941 — Syntax corpus + LSP hover for tie-spread roll](../tickets/open/0941-tracker-tie-span-lsp-hover.md)
- [0942 — Manual update + golden audio tests for tie-spread rolls](../tickets/open/0942-tracker-tie-span-docs-and-goldens.md)

## Acceptance

- `x*3` alone behaves bit-identically to today (no regression on the
  single-tick case).
- `x*3 ~` produces 3 evenly-spaced triggers across two consecutive
  ticks; the second tick fires no independent trigger from the tie
  cell.
- `x*3 ~ ~` produces 3 evenly-spaced triggers across three ticks.
- Gate articulation (currently 80% of interval, gate drops between
  triggers) extends naturally to the longer interval.
- Plain `note ~` and `note ~ ~` (sustain ties, no roll) keep current
  hold-gate / no-trigger semantics — verified by existing
  `tie_holds_gate_and_carries_cv` test plus a new contrast test.
- `just commit -p patches-modules -p patches-dsl -p patches-interpreter`
  green.

## Notes

- The anchor tick's `current_tick_duration_samples` is captured at
  `apply_step` time. Subsequent tick rises during the span do not
  re-run `apply_step` for the absorbed-tie cells, so the in-flight
  schedule survives.
- Swing within a span: v1 accepts a small timing error (interval
  computed from anchor tick only); ADR/follow-up ticket only if this
  is audibly wrong in practice.
- Live-edit / hot-reload: if a tie cell is edited to a non-tie
  mid-roll, the in-flight schedule still completes (uses captured
  interval), but the edited cell's new content will apply on its own
  tick rise. Acceptable.
