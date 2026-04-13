---
id: "0383"
title: Fix CLAP double file read in load_or_parse
priority: low
created: 2026-04-13
---

## Summary

In `patches-clap/src/plugin.rs` around line 600, the file is read into
`p.dsl_source`, then `compile_and_push_plan` calls `load_or_parse` which reads
the same file again from disk. This is wasteful and introduces a TOCTOU race —
if the file changes between the two reads, the stored `dsl_source` and the
compiled plan may disagree.

## Acceptance criteria

- [ ] File is read exactly once per compile cycle
- [ ] `dsl_source` is populated from the same read that `load_or_parse` uses
- [ ] No TOCTOU inconsistency between stored source and compiled plan

## Notes

One approach: have `load_or_parse` return the raw master source alongside the
parsed AST, and store that in `dsl_source`. Alternatively, read into
`dsl_source` first and pass it to the loader as an already-read string.
