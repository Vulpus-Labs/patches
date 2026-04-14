---
id: "0407"
title: Expander play flattening and row-group expansion
priority: high
created: 2026-04-14
---

## Summary

Flatten the new composition syntax into the existing
`SongDef { lanes, rows, loop_point }` shape expected by the interpreter.
Handles row groups, play expressions referencing named sections,
anonymous and named-inline play bodies, and the `@loop` marker.

## Acceptance criteria

- [ ] Row-group repetition `(...) * N` expands into the concatenated
      row list.
- [ ] `play_expr` expansion: terms evaluated left to right; `*` binds
      tighter than `,` (`a, b * 2` → `a, b, b`); parens group for
      repetition (`(a, b) * 2` → `a, b, a, b`).
- [ ] Play atoms are ident references resolved against the song's
      scope chain (song-local sections, then file-level sections).
      Unresolved names are an expansion error.
- [ ] Anonymous inline body (`play { ... }`) emits its rows directly
      without registering a name.
- [ ] Named-inline body (`play foo { ... }`) registers `foo` as a
      song-local section and emits its rows once. Subsequent
      `play foo` (or compositions referencing `foo`) replay it.
      Re-defining `foo` in the same song is an error.
- [ ] Each `play` statement appends to the running row list in source
      order.
- [ ] `@loop` sets `loop_point` to the current row count. Multiple
      `@loop` markers in one song are an error.
- [ ] Every emitted row has exactly `lanes.len()` cells; mismatch is an
      expansion error with span pointing at the offending row.
- [ ] Unit tests covering the acceptance cases and the ADR 0035
      examples.

## Notes

Depends on 0406. The interpreter contract (`FlatPatch` consuming
`Song { channels, order, loop_point }`) is unchanged — this ticket
produces the same shape from richer source.
