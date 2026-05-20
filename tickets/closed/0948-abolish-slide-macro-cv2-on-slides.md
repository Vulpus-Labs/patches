---
id: "0948"
title: Abolish `slide()` macro; `:cv2` on multi-cell slide cells
priority: medium
created: 2026-05-20
epic: E153
depends_on: ["0946"]
---

## Summary

Land the ADR 0077 Amendment 2026-05-20:

- Remove the `slide(n, A, B)` macro generator from grammar, AST,
  expander, parser tests, and fixtures. The four in-tree call sites
  are migrated by hand to the equivalent in-row cell sequence.
- Extend `value>`, `/value`, and `>value` to accept an optional
  `:cv2` tail. The row-build pass populates `SlideOpen.close_cv2`
  from the close cell's `:cv2`; the pattern player ramps cv2
  alongside cv1 with no audio-thread behaviour change for patterns
  that don't use the new tail.
- Keep `value>value` cv1 sugar unchanged (per the amendment's
  "Trade-offs declined" — abolishing it would force every existing
  user of the one-tick slide to reshuffle their row by +1 cell).
- Keep cv2 sugar on triggered notes (`A:0.5>1.0`) unchanged.

This lands before ticket 0947 (LSP / docs polish) so 0947 can
document the final grammar surface in one pass.

## Acceptance criteria

### Grammar

- [ ] `patches-dsl/src/grammar.pest`:
  - Remove `slide_generator` and `slide_endpoint` rules.
  - Collapse `step_or_generator` (now redundant) — either inline
    its single alternative (`step`) at every use site
    (`channel_row`, `channel_row_cont`), or rename it. The AST
    `StepOrGenerator` enum likewise collapses to `Step`.
  - Extend `step_step_to`, `step_slide_open`, `step_slide_close`
    with an optional `(":" ~ cv2_primary)` tail, where
    `cv2_primary = step_unit | step_float | step_int` (same set
    used by `step_cv2`). Compound-atomic `${...}` preserved.
- [ ] `patches-lsp/tree-sitter-patches/grammar.js`:
  - Mirror: remove `slide_generator` / `slide_endpoint` /
    `step_or_generator`; extend the three slide cell rules with
    the optional `:cv2` tail; regenerate `src/grammar.json`,
    `src/parser.c`, `src/node-types.json` via
    `npx tree-sitter generate`.

### AST + parser

- [ ] `patches_dsl::ast`:
  - Remove `StepOrGenerator::Slide` variant; remove
    `StepOrGenerator` enum entirely (replace `Vec<StepOrGenerator>`
    on `PatternChannel.steps` with `Vec<Step>`).
- [ ] `patches_core::StepKind`:
  - `SlideOpen` carries no extra fields (cv2 stays on `Step.cv2`,
    set by parser from the `:cv2` modifier).
  - `StepTo { cv2: Option<f32> }` — already has the field, ensure
    parser populates it.
  - `SlideCloseInTick { cv2: Option<f32> }` — new field.
- [ ] `patches-dsl/src/parser/steps_songs.rs`:
  - Drop `build_slide_generator` / `parse_slide_endpoint` /
    `build_step_or_generator`. `build_channel_row` walks `step`
    pairs directly.
  - Extend `step_slide_open`, `step_step_to`, `step_slide_close`
    parsers to read the optional `:cv2` literal into the kind's
    `cv2` field (or onto `Step.cv2` for `SlideOpen`).
- [ ] `patches-dsl/src/expand/composition.rs`:
  - Remove `Slide` arm in `expand_steps`. The function becomes a
    pass-through (or is inlined where called).

### Row-build + runtime

- [ ] `patches_core::resolve_step_effects`:
  - When a `SlideCloseInTick { cv2 }` cell closes an open slide,
    set `close_cv1` (existing) AND `close_cv2 = cv2` on the open
    `SlideOpen` struct.
  - When a `StepTo { cv2 }` cell closes an open slide, same
    treatment — `close_cv2` carries through.
  - When a triggered close (`StepKind::Note`) closes an open
    slide, `close_cv2` stays `None` (the next StartNote takes
    over; no cv2 ramp through the open slide).
- [ ] `patches-modules/src/sequencer/tracker_core/pattern_player/mod.rs`:
  - `SlideCloseInTick` arm reads the new `cv2: Option<f32>` and,
    when `Some`, ramps cv2 from `self.cv2[ch]` to the supplied
    value across the close tick (mirrors the cv1 ramp logic
    already there).
  - `StartNote { slide: Some(so), … }` and `OpenSlide { slide }`
    arms unchanged — they already read `so.close_cv2`.

### Migration

- [ ] In-tree `slide(n, …)` call sites rewritten:
  - `patches-dsl/tests/fixtures/pattern_slides.patches` — `auto:
    slide(4, 0.0, 1.0)` → `auto: 0.0> >_ >_ >1.0`.
  - `patches-dsl/tests/parser/pattern_song.rs:151,166,183` —
    rewrite the inline strings to the in-row form, and update the
    parser-shape assertions (they currently expect a `Slide`
    variant of `StepOrGenerator`).
  - `patches-interpreter/src/tests/song_sequencer.rs:386` —
    `ch: slide(2, A4, C5)` → `ch: A4> >C5`.
- [ ] `patches-dsl/tests/expand/patterns_songs.rs::expand_slide_generator_produces_steps`:
  rename + rewrite to test the in-row form directly (or delete if
  redundant with new parser tests).
- [ ] Syntax corpus updated:
  - `patches-lsp/tests/syntax_corpus/tracker_step_event_grammar.corpus`:
    add entries for `value:cv2> _ >value:cv2`, `/value:cv2`, and
    `value:cv2> >value:cv2` (one entry per cv2-bearing slide
    shape).

### Tests

- [ ] Parser tests in `patches-dsl/tests/parser/pattern_song.rs`:
  - `step_slide_open_carries_cv2` — `A4:0.5>` sets
    `Step.cv1 = A4`, `Step.cv2 = 0.5`, `kind = SlideOpen`.
  - `step_step_to_carries_cv2` — `/B4:0.8` sets the kind's
    `cv2 = Some(0.8)`.
  - `step_slide_close_carries_cv2` — `>B4:0.8` sets the kind's
    `cv2 = Some(0.8)`.
  - `old_slide_macro_no_longer_parses` — `slide(2, A, B)` is
    rejected at parse time.
- [ ] Row-build tests in `patches-core::tracker::tests`:
  - `slide_open_with_cv2_then_close_in_tick_with_cv2`
    (`A4:0.5> >B4:0.8` → head's `SlideOpen.close_cv2 = Some(0.8)`).
  - `slide_open_with_cv2_then_step_to_with_cv2`
    (`A4:0.5> /B4:0.8` → boundary close, `close_cv2 = Some(0.8)`).
  - `slide_open_with_cv2_then_close_without_cv2` — `close_cv2`
    stays `None`; runtime falls back to open's cv2.
- [ ] Integration tests in `patches-integration-tests::tracker`:
  - `slide_ramps_cv1_and_cv2_simultaneously` — `C4:0.5> _ >C4:1.0`:
    confirm cv1 holds at C4 (start == end) while cv2 ramps
    0.5 → 1.0 continuously across all three ticks.
  - `slide_ramps_cv1_with_cv2_held` — `C4:0.5> _ >G4` (no `:cv2`
    on close): cv1 ramps C4→G4; cv2 holds at 0.5 throughout.
- [ ] `just push` green.

## Notes

- ADR 0077 Amendment 2026-05-20 has the design + worked examples
  and the trade-off table that decided "abolish `slide()` only,
  keep `value>value` sugar".
- The `>_` (TieFlow) cell intentionally does **not** carry cv2 —
  see the amendment's "Trade-offs declined". If you find yourself
  wanting to set cv2 on a flow cell, that's a sign to use
  `>cv1:cv2_end` on the close cell instead.
- The runtime `apply_step`'s `StartNote { slide: Some(so), … }`
  arm already reads `so.close_cv2.unwrap_or(cv2)` — no change
  needed there. The `OpenSlide` arm likewise. Only
  `SlideCloseInTick` needs a new cv2 read.
- Memory note worth saving on close: the asymmetry between
  `value>value` (sugar, 1 cell) and `value> /value` (unsugared,
  2 cells) — the former packs open+close into one tick; the
  latter holds the close value through tick 2. They are NOT
  audibly equivalent across the row.
