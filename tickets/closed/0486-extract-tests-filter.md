---
id: "0486"
title: Extract tests from patches-modules filter.rs
priority: low
created: 2026-04-16
---

## Summary

`patches-modules/src/filter.rs` is 892 lines, of which 307 (34%)
are the inline test module. Extract to a sibling `filter/tests.rs`.

## Acceptance criteria

- [ ] `filter.rs` → `filter/mod.rs` + `filter/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-modules` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
