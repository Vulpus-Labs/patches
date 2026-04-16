---
id: "0476"
title: Extract tests from patches-interpreter lib.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-interpreter/src/lib.rs` is 1510 lines, of which 693 (46%)
are the inline test module. Extract to a sibling `tests.rs` (or
module directory).

## Acceptance criteria

- [ ] Tests live in `patches-interpreter/src/tests.rs` (declared via
      `#[cfg(test)] mod tests;` in `lib.rs`) or equivalent.
- [ ] `cargo test -p patches-interpreter` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change. After extraction,
`lib.rs` impl is ~817 lines — further structural split tracked in
follow-on epic (tier B12).
