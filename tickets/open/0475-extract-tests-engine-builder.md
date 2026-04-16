---
id: "0475"
title: Extract tests from patches-engine builder.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-engine/src/builder.rs` is 1250 lines, of which 714 (57%)
are the inline test module (second `#[cfg(test)]` block at line
537). Extract to a sibling `builder/tests.rs`.

## Acceptance criteria

- [ ] `builder.rs` → `builder/mod.rs` + `builder/tests.rs`.
- [ ] `#[cfg(test)] use` statements at the top of `builder.rs`
      remain or move as appropriate.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-engine` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change. After extraction,
`builder.rs` impl is ~536 lines — further structural split tracked
in follow-on epic (tier B14).
