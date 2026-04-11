---
id: "0312"
title: "Grammar + parser: song blocks"
priority: high
created: 2026-04-11
---

## Summary

Add pest grammar rules and parser logic for `song` blocks — named
arrangements that assign patterns to song-level channels in a row-by-row
order table.

## Acceptance criteria

- [ ] Grammar rule `song_block` as a top-level construct
- [ ] Pipe-delimited table format: `| col1 | col2 | ... |`
- [ ] First row is the header declaring channel names
- [ ] Subsequent rows are pattern name references; `_` denotes silence
- [ ] `@loop` annotation on a row marks the loop point
- [ ] At most one `@loop` per song (parser error if multiple)
- [ ] If no `@loop`, `loop_point` defaults to `0` (loop from beginning)
- [ ] Parser produces `SongDef` AST nodes (from ticket 0308)
- [ ] `file` rule updated to accept `song` blocks (zero or more)
- [ ] Unit tests: basic song, song with `_` silence, song with `@loop`,
      multiple songs in one file
- [ ] `cargo test -p patches-dsl` passes
- [ ] `cargo clippy -p patches-dsl` clean

## Notes

Song blocks only contain pattern name references, not inline step data.
Validation that referenced pattern names actually exist is an interpreter
concern (ticket 0321), not a parser concern.

Song channels are distinct from pattern channels — a song channel
corresponds to one PatternPlayer module instance.

Epic: E057
ADR: 0029
