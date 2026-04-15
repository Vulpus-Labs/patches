---
id: "0425"
title: Surface DSL expansion errors as LSP diagnostics
priority: medium
created: 2026-04-15
---

## Summary

The DSL expander in `patches-dsl` detects a range of structural errors
— including recursive template instantiation, unknown templates, arity
mismatches, and shape-evaluation failures — and returns them as
`ExpandError`. The LSP currently discards these errors silently in
`ensure_flat_locked`, leaving the user with no feedback when a
structurally invalid patch fails to expand.

Publish `ExpandError` as an LSP diagnostic against the authored span it
points to, so editors surface it like any other error.

See epic E078.

## Acceptance criteria

- [ ] `ensure_flat_locked` (`patches-lsp/src/workspace.rs:211-213`) no
      longer discards the `Err(_)` branch. The error is converted to an
      LSP `Diagnostic` with `DiagnosticSeverity::ERROR` and published
      through the existing diagnostics path.
- [ ] Diagnostic is anchored to the `ExpandError`'s source span if
      present; falls back to the whole file otherwise.
- [ ] Recursive-template errors from the expander
      (`patches-dsl/src/expand.rs:1144-1148`) surface with the
      template name in the message.
- [ ] Other `ExpandError` variants (unknown template, arity mismatch,
      shape evaluation) surface with their original messages.
- [ ] Tests cover: self-cycle, mutual cycle, unknown template, and
      `$ -> $` passthrough (`patches-dsl/src/expand.rs:1613-1619`,
      "connection has '$' on both sides"). Assert the LSP publishes a
      diagnostic with the expected span and message.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

Existing expander tests cover the detection itself
(`patches-dsl/tests/expand_tests.rs:239`,
`patches-dsl/tests/torture_tests.rs:350,364`); this ticket only adds the
LSP-side plumbing.

`ExpandError` variants need a span accessor if they don't already
expose one — check before writing the converter. If some variants lack
spans, fall back to the whole file for those until the expander is
extended.
