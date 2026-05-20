---
id: "0939"
title: Annotate Step rows with repeat_span derived from following ties
priority: medium
created: 2026-05-19
closed: 2026-05-19
epic: E152
---

## Summary

Add a `repeat_span: u8` field on the runtime `Step` (or equivalent
row-level annotation) populated at row-build time. For each `*N` step
with `repeat > 1`, span = `1 + (consecutive ties immediately
following)`. The consumed tie cells get an `absorbed_by_roll: bool`
flag so the pattern player skips its normal tie handling on them.

Plain ties (anchor has `repeat == 1`) keep their current sustain
semantics — leave `repeat_span = 1` and `absorbed_by_roll = false`.

## Acceptance criteria

- [x] `Step` (runtime form, not the DSL-AST form) carries
      `repeat_span: u8` (default 1) and `absorbed_by_roll: bool`
      (default false).
- [x] Row finalisation in the tracker pipeline (the layer that turns
      parsed `Step`s into `TrackerData` consumed by the pattern player)
      scans each `*N` anchor forward and sets `repeat_span` on the
      anchor + `absorbed_by_roll` on the consumed tie cells.
- [x] Spans truncate at:
  - end of pattern row (annotation pass operates on the per-channel
    final `Vec<Step>` — runs out of cells at row end);
  - row-continuation (`|`) — **transparent**: parser concatenates
    continuation steps into the same `Vec` before annotation, so a
    span can cross the `|` join (covered by
    `tie_spread_transparent_across_row_continuation`);
  - bank/pattern loop boundary — row-build layer doesn't see the
    loop; pattern player will handle runtime truncation (ticket 0940).
- [x] Unit tests cover the listed scenarios in `patches-core::tracker`
      and an end-to-end set in `patches-interpreter` exercising
      `build_tracker_data` (chained anchors, plain ties unchanged,
      row-end truncation, `|` continuation).
- [x] No change to parser grammar or DSL-AST `Step`. AST stays
      authoring-shape (only added a `Default` impl on `ast::Step` for
      test-side ergonomics); row-build is where the runtime shape
      gets the derived fields.
- [x] `just inner -p patches-interpreter` green (covers
      patches-core / patches-modules / patches-dsp / patches-engine +
      interpreter).

## Resolution

- New fields on `patches_core::tracker::Step`:
  - `repeat_span: u8` (default 1)
  - `absorbed_by_roll: bool` (default false)
  Plus a `Default` impl so existing construction sites can be
  simplified.
- New helper `patches_core::annotate_repeat_spans(&mut [Step])` does
  the row-build scan. Idempotent — resets fields, then walks each
  `*N` anchor forward consuming consecutive tie cells
  (`gate && !trigger`). Called from
  `patches_interpreter::tracker::build_tracker_data` once per channel
  after `convert_step` + rest-padding.
- Pattern player is untouched (no audible behaviour change). Ticket
  0940 consumes the new fields.

## Notes

- Decide carefully where the annotation lives. The parser's `Step` in
  `patches-dsl/src/parser/steps_songs.rs` is the AST form and should
  stay untouched. The runtime `TrackerStep` consumed by the pattern
  player is in `patches-modules/src/tracker_core/`. Annotation belongs
  on the runtime form.
- Treat row-continuation (`|`) as transparent — a span can extend
  across the `|` join because the row is logically one sequence. Test
  this explicitly.
- This ticket lands a no-op on the audible behaviour: the new field is
  populated but ignored by the pattern player. Ticket 0940 consumes
  it.
