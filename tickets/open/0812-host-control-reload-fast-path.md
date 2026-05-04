---
id: "0812"
title: Drop+replace and param-update fast path on manifest change
priority: medium
created: 2026-05-04
epic: E135
depends_on: "0809,0811"
---

## Summary

Distinguish shape changes (add / remove host control) from
metadata-only changes (rename, range, default, taper) and route each
through the right reload path.

## Acceptance criteria

- [ ] Shape change → existing size-change drop+replace path for the
      `~host_control` module instance. CLAP republishes the
      parameter list.
- [ ] Metadata-only change on an unchanged set → pure parameter
      update on the existing instance + CLAP param metadata
      republish. No audio-graph rebuild.
- [ ] Detection compares prior manifest to new; equality on (sorted
      names) → fast path; else drop+replace.
- [ ] Live-reload tests: rename without shape change preserves audio
      continuity (no zipper, no dropout); add new control rebuilds.
- [ ] `just inner -p patches-engine -p patches-clap` passes.

## Notes

- The shape-change path already exists for templates and taps; this
  is a wiring task, not new infrastructure.
