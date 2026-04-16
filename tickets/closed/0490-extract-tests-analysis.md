---
id: "0490"
title: Extract tests from patches-lsp analysis/mod.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-lsp/src/analysis/mod.rs` is 942 lines, of which 788 (84%)
are the inline `#[cfg(test)] mod tests { ... }`. The orchestrator is
154 lines; opening the file to understand the analysis pipeline
means scrolling past a wall of tests.

Extract the test module to a sibling `tests.rs`, matching the
pattern established by ticket 0459 for `workspace.rs`.

## Acceptance criteria

- [ ] `patches-lsp/src/analysis/tests.rs` holds the former inline
      test module contents.
- [ ] `analysis/mod.rs` declares `#[cfg(test)] mod tests;` and
      nothing else test-related.
- [ ] `cargo test -p patches-lsp` passes with the same test count
      as before.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
