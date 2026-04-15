---
id: "0458"
title: Move peek code action out of server.rs
priority: low
created: 2026-04-15
---

## Summary

`patches-lsp/src/server.rs` lines 200–222 contain the
`patches.peekExpansion` code-action construction. This is a feature,
not LSP-protocol boilerplate, and should live next to the rest of
peek logic in `peek.rs` (or a new `actions.rs`).

## Acceptance criteria

- [ ] Peek code-action construction lives in `peek.rs` as a public
      function.
- [ ] `server.rs` calls it from the `code_action` handler.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E083. Trivial move; keeps `server.rs` as protocol glue only.
