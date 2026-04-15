---
id: "0455"
title: Make song flatten→resolve dependency explicit
priority: low
created: 2026-04-15
---

## Summary

Song assembly in `expand.rs` runs in two passes with an implicit
ordering: `flatten_song` (lines 399–512) produces `AssembledSong` with
raw `SongCell`s, then `resolve_songs` (lines 170–220) converts cells
to `PatternIdx`. They are separated by pattern sorting (line 150), and
each repeats similar error checks (e.g. pattern-not-found at 191–194
vs. 289–294). Merge the passes, add a typed intermediate that encodes
"sorted, unresolved", or factor the shared error path.

## Acceptance criteria

- [ ] Dependency between flatten and resolve is either removed (merged
      pass) or encoded in the type system (e.g. `UnresolvedSong` →
      `ResolvedSong`).
- [ ] Pattern-not-found error handling is single-sourced.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

E083. Fits naturally with 0450's composition module.
