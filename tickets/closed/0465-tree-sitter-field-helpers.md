---
id: "0465"
title: Tree-sitter field extraction helpers
priority: medium
created: 2026-04-15
---

## Summary

Feature handlers repeatedly extract tree-sitter node fields by
name and check against magic strings. Examples:

- `MasterSequencer` literal check at
  `patches-lsp/src/completions.rs:108` and `:132`
- `module_type` field extraction at `hover.rs:130`
- Same `child_by_field_name` + `node_text` pattern duplicated
  across completions, hover, navigation

A field rename or module rename requires hunting through every
handler. The walker leaks to callers.

## Acceptance criteria

- [ ] Helpers in `tree_nav` (or new `tree_util`) covering the
      common queries: `module_type_name(module_decl)`,
      `param_value_for_name(param_block, param_name)`, etc.
- [ ] Magic string literals (`"MasterSequencer"`, field names)
      localised — handlers call helpers, not walkers.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E084. Pairs with 0463 — both reduce handler-side tree-sitter
literacy.
