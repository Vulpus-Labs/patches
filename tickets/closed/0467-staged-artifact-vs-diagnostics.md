---
id: "0467"
title: Separate StagedArtifact caching from diagnostic rendering
priority: low
created: 2026-04-15
---

## Summary

`patches-lsp/src/workspace/mod.rs:38-70` defines `StagedArtifact`
holding `flat`, `references`, `signal_graph`, `source_map`,
`bound`, plus pre-bucketed `Vec<(Url, Vec<Diagnostic>)>`. The
diagnostic bucketing logic lives in `run_pipeline_locked` (lines
247–264) and calls helpers like `merge_signal_graph_warnings`.

This mixes rendering (which Diagnostic strings) with caching
(which artifact fields exist). A reader can't tell whether
diagnostics are pre-rendered or need reconstruction. Adding a
new pipeline phase requires editing both the artifact shape
and the bucketing.

## Acceptance criteria

- [ ] `StagedArtifact` holds patch artifacts only (flat, bound,
      source_map, references, signal_graph) — no diagnostics.
- [ ] Diagnostics rendered lazily by `analyse()` /
      `run_pipeline_locked` from artifact contents at
      publish-time.
- [ ] No behaviour change in published diagnostics.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E084. Related to 0436/0439 (diagnostic bucketing in the
pipeline orchestrator). This ticket is the LSP-side analogue:
keep the artifact pure, render at the edge.
