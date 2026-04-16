---
id: "0480"
title: Extract tests from patches-modules poly_filter.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-modules/src/poly_filter.rs` is 980 lines, of which 439
(45%) are the inline test module. Extract to a sibling
`poly_filter/tests.rs`.

## Acceptance criteria

- [ ] `poly_filter.rs` → `poly_filter/mod.rs` +
      `poly_filter/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-modules` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
