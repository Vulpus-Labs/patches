---
id: "0457"
title: Unified tree_nav helper for hover and completions
priority: medium
created: 2026-04-15
---

## Summary

`hover.rs` and `completions.rs` each re-implement tree-sitter navigation
in different idioms — hover uses a `try_hover_*` prefix with
ancestor-chain walking (hover.rs:29–56), completions uses a cursor loop
with match arms (completions.rs:81–110). Both work, but a sixth
handler author will not know which pattern to follow. Extract a shared
helper that classifies cursor context once; each handler matches on
the classification.

## Acceptance criteria

- [ ] New `patches-lsp/src/tree_nav.rs` exposes a cursor-context query
      returning an enum (e.g. `QueryContext::ModuleType`,
      `PortRef`, `TemplateArg`, …).
- [ ] `hover.rs` and `completions.rs` call the helper and match on
      the result instead of re-walking the tree.
- [ ] `inlay.rs` and `navigation.rs` use it where applicable.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E083. Not huge (~100 lines of new helper), but pays back on every new
handler.
