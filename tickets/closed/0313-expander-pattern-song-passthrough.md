---
id: "0313"
title: "Expander: pattern and song pass-through to FlatPatch"
priority: high
created: 2026-04-11
---

## Summary

Extend the template expander and `FlatPatch` to carry pattern and song
definitions through to the interpreter. Pattern and song blocks are not
affected by template expansion — they pass through unchanged.

## Acceptance criteria

- [ ] `FlatPatch` gains `patterns: Vec<PatternDef>` and
      `songs: Vec<SongDef>` fields (or equivalent flat representations)
- [ ] `expand()` copies pattern and song definitions from the parsed
      `File` into the `FlatPatch`
- [ ] `slide()` generators (if not already expanded at parse time) are
      expanded into concrete step sequences during this pass
- [ ] Round-trip test: parse a file with patterns, songs, templates, and
      a patch block; expand; verify patterns and songs are preserved in
      the `FlatPatch`
- [ ] `cargo test -p patches-dsl` passes
- [ ] `cargo clippy -p patches-dsl` clean

## Notes

The expander's primary job is template instantiation, which doesn't
interact with pattern/song blocks. This ticket is mainly about plumbing
the new data through the existing expand pipeline.

If the flat representation differs from the AST representation (e.g.
resolved step data vs. raw notation), define the flat types here.
Otherwise, reuse the AST types directly.

Epic: E057
ADR: 0029
