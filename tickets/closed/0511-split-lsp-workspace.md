---
id: "0511"
title: Split patches-lsp workspace/mod.rs by concern
priority: medium
created: 2026-04-16
---

## Summary

`patches-lsp/src/workspace/mod.rs` is 1110 lines. `include_graph`
already lives alongside; the remaining file bundles document
lifecycle, pipeline orchestration, feature-handler plumbing
(`with_expansion_context` / `ExpansionCtx`), and diagnostic
publish bookkeeping.

## Acceptance criteria

- [ ] Add sibling submodules:
      `lifecycle.rs` (open/change/close/document state),
      `analysis.rs` (run_pipeline_locked + StagedArtifact plumbing),
      `features.rs` (with_expansion_context, ExpansionCtx),
      `publish.rs` (non-root publish-URI tracking).
- [ ] `DocumentWorkspace` struct + top-level methods stay in
      `mod.rs`; impl blocks scoped per submodule via
      `impl DocumentWorkspace { ... }`.
- [ ] `mod.rs` under ~500 lines.
- [ ] `cargo build -p patches-lsp`, `cargo test -p patches-lsp`,
      `cargo clippy` clean.

## Notes

E086. Server/LSP surface unchanged.
