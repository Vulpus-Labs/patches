---
id: "0267"
title: Tree-sitter CST to tolerant AST builder
priority: high
created: 2026-04-07
---

## Summary

Implement a tree-sitter CST walker that produces the tolerant AST, accumulating
diagnostics for ERROR and MISSING nodes encountered during the walk.

## Acceptance criteria

- [ ] `patches-lsp/src/ast_builder.rs` provides a function
      `build_ast(tree: &Tree, source: &str) -> (File, Vec<Diagnostic>)` (or
      similar) that walks the tree-sitter CST and produces the tolerant AST.
- [ ] ERROR nodes in the CST produce diagnostics with the error span and a
      "syntax error" message. The builder continues past errors.
- [ ] MISSING nodes result in `None` for the corresponding `Option<T>` field
      and a diagnostic.
- [ ] Valid `.patches` files produce the same logical structure as the pest
      parser (verified by comparison tests on key fixtures).
- [ ] Numeric conversions match `patches-dsl` semantics: note literals to
      v/oct, Hz/kHz to v/oct, dB to linear amplitude.
- [ ] Tests cover: a fully valid file producing zero diagnostics, a file with
      a missing module type name, a file with an unclosed param block.

## Notes

- The builder is a recursive descent walk over named CST nodes. Each
  `build_foo(node)` method extracts child nodes by field name and constructs
  the tolerant AST node.
- Numeric conversion logic can be extracted into a shared utility or
  reimplemented — it's small (~20 lines for note/Hz/dB conversions).
- Epic: E049
