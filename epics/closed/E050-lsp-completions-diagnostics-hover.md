---
id: "E050"
title: "LSP completions, diagnostics, and hover"
created: 2026-04-07
tickets: ["0270", "0271", "0272", "0273"]
---

## Summary

Wire the semantic analysis pipeline into the tower-lsp server to deliver
diagnostics, completions, and hover information to the editor. By the end of
this epic the LSP is functional end-to-end: editing a `.patches` file in
VS Code shows inline errors, offers context-sensitive completions, and
displays module metadata on hover.

See ADR 0024 for architectural context.

## Tickets

| ID   | Title                                          | Priority | Depends on |
|------|-------------------------------------------------|----------|------------|
| 0270 | Document sync and incremental reparse              | high | 0269       |
| 0271 | Publish diagnostics on document change             | high | 0270       |
| 0272 | Context-sensitive completions                      | high | 0270       |
| 0273 | Hover information for modules and ports            | medium | 0270      |

## Definition of done

- The LSP server tracks open documents, incrementally reparses on each edit
  using tree-sitter's edit API, and maintains a stable semantic model.
- Diagnostics (unknown module types, invalid params, bad connections) appear
  inline in the editor on save or as-you-type.
- Completions are offered for: module type names (after `:`), shape arg names
  (inside `()`), parameter names (inside `{}`), and port names (after `.` in
  connections).
- Hover on a module type name shows its ports and parameters with types/ranges.
  Hover on a port reference shows the port kind (mono/poly).
- All features tested with both unit tests and manual verification in the
  VS Code extension development host.
- `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.
