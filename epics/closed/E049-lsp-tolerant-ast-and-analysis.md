---
id: "E049"
title: "Tolerant AST and semantic analysis"
created: 2026-04-07
tickets: ["0266", "0267", "0268", "0269"]
---

## Summary

Build the tolerant AST types, the tree-sitter CST-to-AST builder, and the
four-phase semantic analysis pipeline. By the end of this epic the LSP can
parse a `.patches` file, extract module/template/enum declarations, resolve
module descriptors via the registry, and validate connections and parameters —
accumulating diagnostics throughout.

See ADR 0024 for architectural context.

## Tickets

| ID   | Title                                          | Priority | Depends on |
|------|-------------------------------------------------|----------|------------|
| 0266 | Tolerant AST types                                 | high | 0264       |
| 0267 | Tree-sitter CST to tolerant AST builder            | high | 0266       |
| 0268 | Shallow scan and dependency resolution (phases 1-2)| high | 0267       |
| 0269 | Descriptor instantiation and body analysis (phases 3-4) | high | 0268 |

## Definition of done

- Tolerant AST types mirror the DSL's structure with `Option<T>` fields for
  error tolerance.
- The CST builder produces a tolerant AST from any tree-sitter parse (including
  parses with ERROR/MISSING nodes), accumulating diagnostics.
- Semantic analysis resolves module descriptors via `Registry::describe()` and
  validates parameter names/types and connection port names/indices.
- Analysis emits diagnostics with source spans for: unknown module types,
  unknown parameters, type mismatches, unknown ports, invalid indices, and
  template dependency cycles.
- Tests cover both valid files and files with deliberate errors.
- `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.
