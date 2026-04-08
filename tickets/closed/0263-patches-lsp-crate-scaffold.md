---
id: "0263"
title: patches-lsp crate scaffold with tower-lsp
priority: high
created: 2026-04-07
---

## Summary

Create the `patches-lsp` Rust crate with a tower-lsp server that responds to
`initialize` and `shutdown`. This is the skeleton that all subsequent LSP
functionality builds on.

## Acceptance criteria

- [ ] `patches-lsp/Cargo.toml` with dependencies on `tower-lsp`, `tree-sitter`,
      `patches-core`, and `patches-modules`.
- [ ] `patches-lsp/src/main.rs` — binary entry point that starts the tower-lsp
      server on stdio.
- [ ] `patches-lsp/src/server.rs` — `LanguageServer` impl that handles
      `initialize` (advertises capabilities) and `shutdown`.
- [ ] The crate is added to the workspace `Cargo.toml`.
- [ ] `cargo build -p patches-lsp` produces a `patches-lsp` binary.
- [ ] The binary starts, accepts an LSP `initialize` request on stdin, and
      responds correctly.
- [ ] `cargo clippy -p patches-lsp` passes clean.

## Notes

- Capabilities advertised in `initialize` will be expanded in later tickets.
  Start with `text_document_sync: Full` and empty completion/hover providers.
- Epic: E048
