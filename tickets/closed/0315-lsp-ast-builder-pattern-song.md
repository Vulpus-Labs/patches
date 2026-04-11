---
id: "0315"
title: "LSP AST builder: pattern and song nodes"
priority: medium
created: 2026-04-11
---

## Summary

Extend the LSP's tolerant AST builder (`patches-lsp/src/ast_builder.rs`)
to construct AST nodes from the tree-sitter parse tree for pattern and
song blocks. These nodes feed into completions, diagnostics, and hover.

## Acceptance criteria

- [ ] LSP AST types for pattern blocks: name, channel names, step count
- [ ] LSP AST types for song blocks: name, channel names, pattern
      references per row, loop point
- [ ] `ast_builder` walks `pattern_block` and `song_block` tree-sitter
      nodes and produces the corresponding AST nodes
- [ ] Pattern and song names are collected into the document-level symbol
      table
- [ ] Pattern name references inside song rows are tracked with their
      source ranges (for go-to-definition and diagnostics)
- [ ] Unit tests: AST construction from valid pattern/song blocks,
      graceful handling of incomplete blocks
- [ ] `cargo test -p patches-lsp` passes
- [ ] `cargo clippy -p patches-lsp` clean

## Notes

The LSP AST is separate from the patches-dsl AST — it's optimised for
editor features (source ranges, partial parse tolerance) rather than
execution. Follow the existing patterns in `ast_builder.rs` for how
template and patch blocks are handled.

Epic: E058
ADR: 0029
