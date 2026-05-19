---
id: "0939"
title: Annotate Step rows with repeat_span derived from following ties
priority: medium
created: 2026-05-19
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

- [ ] `Step` (runtime form, not the DSL-AST form) carries
      `repeat_span: u8` (default 1) and `absorbed_by_roll: bool`
      (default false).
- [ ] Row finalisation in the tracker pipeline (the layer that turns
      parsed `Step`s into `TrackerData` consumed by the pattern player)
      scans each `*N` anchor forward and sets `repeat_span` on the
      anchor + `absorbed_by_roll` on the consumed tie cells.
- [ ] Spans truncate at:
  - end of pattern row,
  - row-continuation (`|`) boundary if treated as logical end (decide
    + document one way),
  - bank/pattern loop boundary (the row-build layer doesn't see the
    loop, so just don't span past row end — pattern player handles
    runtime truncation if needed).
- [ ] Unit tests cover: `x*3` alone (span=1), `x*3 ~` (span=2), `x*5 ~ ~`
      (span=3), `note ~` (no change — anchor has repeat=1), `x*3 ~ note`
      (span=2; the `note` is not absorbed), `x*3 ~ ~ ~ note*2 ~`
      (anchor 1 span=4, anchor 2 span=2).
- [ ] No change to parser grammar or DSL-AST `Step`. AST stays
      authoring-shape; row-build is where the runtime shape gets the
      derived fields.
- [ ] `just inner -p patches-modules -p patches-interpreter` green.

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
