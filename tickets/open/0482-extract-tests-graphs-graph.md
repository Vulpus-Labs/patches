---
id: "0482"
title: Extract tests from patches-core graphs/graph.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-core/src/graphs/graph.rs` is 766 lines, of which 394 (51%)
are the inline test module. Extract to a sibling `graph/tests.rs`.

## Acceptance criteria

- [ ] `graphs/graph.rs` → `graphs/graph/mod.rs` +
      `graphs/graph/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-core` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
