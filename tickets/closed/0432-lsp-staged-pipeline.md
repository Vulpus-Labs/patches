---
id: "0432"
title: Migrate patches-lsp primary path to staged pipeline
priority: high
created: 2026-04-15
---

## Summary

Rewrite `patches-lsp::workspace::DocumentWorkspace` so the primary
code path runs the orchestrator (stages 1–5) under an
accumulate-and-continue policy rather than the current ad-hoc
`maybe_parse_pest` → expansion → partial binding flow. Feature
handlers (hover, completions, analysis) consume the bound graph from
stage 5 or the partial FlatPatch from earlier stages when later stages
failed. Diagnostics from every stage that ran are aggregated and
published as one set per document.

## Acceptance criteria

- [ ] `DocumentWorkspace::ensure_flat_locked` replaced by a staged
      orchestrator call; `flat_cache` now stores the bound graph and
      accumulated diagnostics, not just `FlatPatch`.
- [ ] `invalidate_flat_closure` invalidates the cached artifact and
      diagnostics together.
- [ ] `PatchReferences` (ADR 0037) still built alongside the bound
      graph; lifetime unchanged.
- [ ] One `publishDiagnostics` call per document covers stages 1–5.
- [ ] Feature handlers in `hover.rs`, `completions.rs`, `analysis.rs`,
      `expansion.rs` consume the new artifact type; no handler reaches
      into per-stage internals.
- [ ] `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

Depends on E079. Does **not** include the tree-sitter fallback gating
— that's 0433. Until 0433 lands the tree-sitter path keeps running as
today in parallel; this ticket just ensures the pest path goes
through the orchestrator.
