---
id: "0459"
title: Split workspace.rs into state, features, and tests
priority: medium
created: 2026-04-15
---

## Summary

`patches-lsp/src/workspace.rs` is 2432 lines and conflates three roles:
state management (`DocumentState`, `WorkspaceState`,
`DocumentWorkspace`), feature implementations (`analyse`, `completions`,
`hover`, `goto_definition`, etc. at lines 300–680 and scattered),
and ~1250 lines of end-to-end tests (lines 1173–2432). Readers opening
the file to understand state layout must skip past the feature
implementations; feature authors must hunt state through tests. Split.

## Acceptance criteria

- [ ] State types and public API live in `workspace/state.rs` (or
      `workspace.rs` trimmed).
- [ ] Feature implementations live in `workspace/features.rs` or split
      per feature.
- [ ] Integration tests move to `patches-lsp/tests/` or an internal
      `workspace/tests.rs`.
- [ ] Public surface of the crate unchanged.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E083. Low urgency — code is correct — but forward-looking hygiene as
more LSP features land (rename, signature help, document symbols).
