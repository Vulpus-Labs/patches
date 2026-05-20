---
id: "0941"
title: Syntax corpus + LSP hover for tie-spread roll
priority: low
created: 2026-05-19
closed: 2026-05-20
epic: E152
depends_on: ["0939"]
---

## Summary

Add a `patches-lsp/tests/syntax_corpus/` entry for tie-spread roll
patterns and surface the disambiguated tie meaning in LSP hover.

Per syntax-corpus policy, any change to how a syntactic construct is
*interpreted* — even without a grammar change — warrants a corpus
entry so future grammar/lower passes don't silently regress it.

## Acceptance criteria

- [x] New corpus file
      `patches-lsp/tests/syntax_corpus/tracker_tie_spread.corpus` with
      eight entries covering: bare `*N`, `*N ~`, `*N ~ ~`, chained
      anchors, plain-tie sustain, the multi-channel mix the ticket
      called out, `|` row-continuation transparency, and `*N` combined
      with cv2 + slide.
      The corpus harness is parse-shape only (`tag = ok` ⇒ pest +
      tree-sitter accept). Annotated-lower assertions live in the
      `patches-core::tracker::tests::annotate_*` suite (ticket 0939)
      and the `patches-interpreter::tests::song_sequencer::tie_spread_*`
      suite — together they pin down `repeat`, `repeat_span`, and
      `absorbed_by_roll` end-to-end.
- [x] LSP hover on a tie cell distinguishes the two cases —
      `hover_for_tie` walks the channel-row siblings, runs
      `annotate_repeat_spans`, and renders:
  - plain tie ⇒ "**`~` (sustain tie)** — Hold the gate high; emit no
    new trigger. cv1/cv2 carry over from the previous step."
  - absorbed tie ⇒ "**`~` (roll continuation)** — Extends the
    preceding `*N` roll across this tick (E152 tie-spread). …"
- [x] LSP hover on a `*N` anchor reports the derived span —
      `hover_for_repeat` shows "rolled across N ticks" for `span > 1`
      and "single-tick roll" otherwise. Covered by
      `hover_on_repeat_anchor_with_span_reports_span` and
      `hover_on_single_tick_repeat_anchor_describes_subdivision`.
- [x] `cargo test -p patches-lsp` green (183 tests, including the new
      corpus entry and four new hover tests).

## Resolution

- New `CursorContext::StepTie` and `CursorContext::StepRepeat`
  variants in [`tree_nav.rs`](../../patches-lsp/src/tree_nav.rs); a
  `classify_step_node` helper walks the cursor's ancestors to the
  enclosing `channel_row` (the channel-row continuation rule
  `channel_row_cont` is flattened during the scan, so spans crossing
  the `|` join are visible to hover).
- New [`hover/step.rs`](../../patches-lsp/src/hover/step.rs) decodes
  each tree-sitter `step` node into a runtime `TrackerStep`, calls
  the shared `patches_core::annotate_repeat_spans` over the row, and
  reads `repeat_span` / `absorbed_by_roll` to pick hover wording.
  Reusing the row-build helper means LSP and runtime can never
  disagree on which ties are absorbed.
- Completions had to learn the new variants too (added to the
  pass-through arm in `completions/mod.rs`).
- The hover assertion strings keep the markdown bold sparingly so
  substring tests don't trip over emphasis tokens.

## Notes

- The acceptance bullet about "expected lower output captured in the
  corpus" was reinterpreted: the corpus harness has only parse-shape
  tags (`ok` / `pest_error` / `ts_error` / `both_error` /
  `expand_error`) and no "annotated lower" tag. Rather than build a
  bespoke lower-snapshot mechanism for one feature, the annotation
  shape is locked in by the dedicated unit tests; the corpus locks
  the syntactic shape those tests depend on so future grammar work
  doesn't silently shift it.

## Original notes

- This ticket only fires after 0939 because the LSP hover needs the
  row-build annotations to know which interpretation applies. If the
  LSP runs a separate lower pass that doesn't share the row-build
  code, factor the span-derivation helper out of 0939's row-build into
  a small shared function.
