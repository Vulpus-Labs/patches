---
id: "0424"
title: LSP signal graph and unused-output diagnostics
priority: medium
created: 2026-04-15
---

## Summary

Build a per-root `SignalGraph` alongside `PatchReferences` in
`ensure_flat_locked`, indexing forward and reverse adjacency keyed by
`(QName, PortLabel)`. Use it to publish unused-output diagnostics:
flat module output ports with no downstream connection.

See ADR 0037 and epic E078.

## Acceptance criteria

- [ ] New `patches-lsp::signal_graph::SignalGraph` struct with:
      - `forward: HashMap<(QName, PortLabel), Vec<(QName, PortLabel)>>`
      - `reverse: HashMap<(QName, PortLabel), Vec<(QName, PortLabel)>>`
- [ ] Built from `(FlatPatch, &PatchReferences)` in `ensure_flat_locked`,
      cached with the same lifetime as `flat_cache` and
      `PatchReferences`, invalidated together.
- [ ] Unused-output diagnostic: any `FlatModule` output port absent
      from `forward` produces a warning at the authored span of the
      module (resolved via `Provenance`). Top-level patch outputs are
      excluded.
- [ ] Diagnostic published through the existing diagnostics path in
      `workspace.rs`; no new LSP capability needed.
- [ ] Tests cover: module with all outputs connected (no diagnostic),
      module with one unused output (one diagnostic), top-level output
      not flagged, fan-out target counted correctly.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

No cycle detection here. Signal-level cycles are fine (1-sample cable
delay, per CLAUDE.md "Parallelism-ready execution") at every
granularity — module, template instance, or otherwise. Template
*instantiation* cycles are a separate concern handled by the DSL
expander and covered by ticket 0425.

`SignalGraph` is sibling-to rather than part-of `PatchReferences`
because:

- Its size scales with connection count and is larger than the existing
  index.
- Its consumers (diagnostics, future graph features) differ from the
  hover/inlay/peek consumers of `PatchReferences`.
- Keeping them separate lets `PatchReferences` stay cheap for the
  interactive query path even when the graph grows.
