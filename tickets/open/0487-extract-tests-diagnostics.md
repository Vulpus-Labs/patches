---
id: "0487"
title: Extract tests from patches-diagnostics lib.rs
priority: low
created: 2026-04-16
---

## Summary

`patches-diagnostics/src/lib.rs` is 691 lines, of which 291 (42%)
are the inline test module. Extract to a sibling `tests.rs`.

## Acceptance criteria

- [ ] Tests live in `patches-diagnostics/src/tests.rs` (declared
      via `#[cfg(test)] mod tests;` in `lib.rs`).
- [ ] `cargo test -p patches-diagnostics` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
