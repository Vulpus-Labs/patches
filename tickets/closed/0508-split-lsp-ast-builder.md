---
id: "0508"
title: Split patches-lsp ast_builder.rs by AST section
priority: medium
created: 2026-04-16
---

## Summary

`patches-lsp/src/ast_builder.rs` is 1465 lines of `build_*`
functions that walk the tree-sitter parse tree and lower it to the
LSP tolerant AST. Functions cluster cleanly by AST section.

## Acceptance criteria

- [ ] Convert to `ast_builder/mod.rs` with submodules roughly:
      `file_patch.rs` (file/patch/statements), `module_decl.rs`
      (module_decl, shape_block, shape_arg, alias_list),
      `params.rs` (param_block, param_entry, param_index,
      at_block), `song_pattern.rs`, `literals.rs` (notes, Hz,
      numbers, strings), `diagnostics.rs` (Diagnostic,
      DiagnosticKind, Severity, error walking).
- [ ] `build_ast` entry point stays in `mod.rs`.
- [ ] Each submodule under ~400 lines; `mod.rs` under ~300.
- [ ] `cargo build -p patches-lsp`, `cargo test -p patches-lsp`,
      `cargo clippy` clean.

## Notes

E086. Private helpers; only `build_ast` / `Diagnostic` are used
outside.
