---
id: "0279"
title: Extract hover logic from server.rs
priority: high
created: 2026-04-08
---

## Summary

The hover logic in `server.rs` (~300 lines) is the second largest concern after
completions. Extract it into a dedicated `hover.rs` module.

## Acceptance criteria

- [ ] New module `patches-lsp/src/hover.rs` exists.
- [ ] `compute_hover` and all its helpers (`try_hover_module_type`,
      `try_hover_port`, `try_hover_module_name`, `format_module_descriptor_hover`,
      `format_template_hover`, `format_parameter_kind`, `cable_kind_str`,
      `node_to_range`) move to `hover.rs`.
- [ ] `server.rs` calls `hover::compute_hover(...)` from the `hover` method.
- [ ] All existing hover tests move to `hover.rs::tests`.
- [ ] `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.

## Notes

Depends on T-0278 so that shared helpers (tree-sitter node utilities,
`find_ancestor`, etc.) are already factored out and can be reused here.
