---
id: "E074"
title: Song sections and play composition
created: 2026-04-14
tickets: ["0404", "0405", "0406", "0407", "0408", "0409"]
---

## Summary

Replace the pipe-table `song` body with a composition-oriented syntax:
lanes declared in the song header, named `section` blocks, a `play`
statement language supporting repetition and grouping, inline section
and pattern definitions, and a scoping model that keeps song-local
definitions private. See ADR 0035.

## Acceptance criteria

- [ ] Pest grammar rewritten: `song(lanes)`, `section`, `play` with
      expression syntax, named/anonymous inline blocks, row groups
      `(...) * N`, `@loop` marker, inline `pattern` blocks in songs,
      top-level `section` blocks. Pipe-table song body removed.
- [ ] AST nodes for sections, play expressions, row groups, song items.
- [ ] Expander implements the scope model (song / template / file) with
      song-local pattern name mangling via `QName` (E073).
- [ ] Expander flattens play composition into the existing
      `SongDef { lanes, rows, loop_point }` so the interpreter contract
      is unchanged.
- [ ] Fixtures and unit tests cover: nested row groups, play
      composition (`,`, `*`, groups), `@loop`, inline patterns,
      top-level sections, cross-song scoping isolation, lane-count
      mismatch errors, bare-cell `* N` rejection, and rejection of
      inline row blocks inside `play`.
- [ ] LSP updates: hover and go-to-definition for section names.
- [ ] Manual (`docs/src/`) updated with the new syntax; ADR 0029 cross-
      references ADR 0035.
- [ ] `cargo test` passes across the workspace; `cargo clippy` clean.
- [ ] All tickets closed.

## Tickets

| ID   | Title                                                     |
| ---- | --------------------------------------------------------- |
| 0404 | Grammar for song lanes, sections, play expressions        |
| 0405 | AST for sections, play composition, song items            |
| 0406 | Expander scope model and song-local pattern mangling      |
| 0407 | Expander play flattening and row-group expansion          |
| 0408 | Update fixtures, manual, and LSP for new song syntax      |
| 0409 | Remove pipe-table song body and migrate existing fixtures |

## Notes

Depends on E073 (`QName`) for song-local pattern mangling. ADR 0035
supersedes the song-block portion of ADR 0029 but leaves the pattern,
`MasterSequencer`, and `PatternPlayer` portions unchanged.
