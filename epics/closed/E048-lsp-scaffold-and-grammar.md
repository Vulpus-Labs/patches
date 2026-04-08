---
id: "E048"
title: "LSP scaffold, VS Code extension, and tree-sitter grammar"
created: 2026-04-07
tickets: ["0262", "0263", "0264", "0265"]
---

## Summary

Bootstrap the `patches-lsp` crate and VS Code extension, then implement the
tree-sitter grammar for the patches DSL. By the end of this epic there is a
working development loop: F5 in VS Code launches the extension, the LSP binary
starts, and tree-sitter can parse all existing `.patches` files into CSTs.

See ADR 0024 for architectural context.

## Tickets

| ID   | Title                                          | Priority | Depends on |
|------|-------------------------------------------------|----------|------------|
| 0262 | VS Code extension scaffold with syntax highlighting | high | —          |
| 0263 | `patches-lsp` crate scaffold with tower-lsp        | high | —          |
| 0264 | Tree-sitter grammar for patches DSL                | high | 0263       |
| 0265 | Corpus tests from existing fixture and example files | high | 0264     |

## Definition of done

- `patches-vscode/` contains a VS Code extension with TextMate syntax
  highlighting and LSP client configuration. F5 launches the extension
  development host and connects to the `patches-lsp` binary.
- `patches-lsp` crate builds, starts a tower-lsp server, and responds to
  `initialize` / `shutdown`.
- Tree-sitter grammar parses all valid `.patches` files in `examples/` and
  `patches-dsl/tests/fixtures/` into CSTs with no ERROR nodes.
- Corpus tests cover all major syntax constructs.
- `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.
