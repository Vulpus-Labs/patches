---
id: "0484"
title: Extract tests from patches-core graphs/planner/mod.rs
priority: low
created: 2026-04-16
---

## Summary

`patches-core/src/graphs/planner/mod.rs` is 604 lines, of which 349
(58%) are the inline test module. Extract to a sibling
`planner/tests.rs` (directory already exists).

## Acceptance criteria

- [ ] `graphs/planner/tests.rs` holds the inline test module.
- [ ] `planner/mod.rs` declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-core` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change. Directory
already exists so no file-to-directory conversion needed.
