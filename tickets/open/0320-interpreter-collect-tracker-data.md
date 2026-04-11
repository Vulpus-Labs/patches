---
id: "0320"
title: "Interpreter: collect pattern/song blocks, build TrackerData"
priority: high
created: 2026-04-11
---

## Summary

Extend the interpreter to process pattern and song definitions from the
`FlatPatch`, resolve pattern name references to bank indices, and
construct the `TrackerData` that will be attached to the execution plan.

## Acceptance criteria

- [ ] Interpreter reads `FlatPatch.patterns` and builds `Pattern` structs
      (runtime format from ticket 0318)
- [ ] Pattern bank indices assigned by alphabetical sort on pattern names
- [ ] Interpreter reads `FlatPatch.songs` and builds `Song` structs,
      resolving pattern name references to bank indices
- [ ] `_` in song rows produces a sentinel index (e.g. `usize::MAX`) or
      `Option<usize>` indicating silence
- [ ] `TrackerData` is constructed and returned alongside the
      `ModuleGraph`
- [ ] `build()` function signature updated to return tracker data (or
      the `ModuleGraph` is extended to carry it)
- [ ] Unit tests: build tracker data from a FlatPatch with patterns and
      songs; verify bank indices, order table, loop points
- [ ] `cargo test -p patches-interpreter` passes
- [ ] `cargo clippy -p patches-interpreter` clean

## Notes

This ticket handles the "happy path" — converting parsed data to runtime
structures. Validation (ticket 0321) adds error checking on top.

The interpreter already depends on patches-core and patches-dsl, so no
new crate dependencies are needed.

Epic: E059
ADR: 0029
