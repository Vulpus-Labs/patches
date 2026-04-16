---
id: "0479"
title: Extract tests from patches-modules pattern_player.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-modules/src/pattern_player.rs` is 860 lines, of which 451
(52%) are the inline test module. Extract to a sibling
`pattern_player/tests.rs`.

## Acceptance criteria

- [ ] `pattern_player.rs` → `pattern_player/mod.rs` +
      `pattern_player/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-modules` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
