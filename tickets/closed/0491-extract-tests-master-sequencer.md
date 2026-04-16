---
id: "0491"
title: Extract tests from patches-modules master_sequencer.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-modules/src/master_sequencer.rs` is 1404 lines, of which
777 (55%) are the inline test module. Extract to a sibling
`master_sequencer/tests.rs`.

## Acceptance criteria

- [ ] `master_sequencer.rs` → `master_sequencer/mod.rs` +
      `master_sequencer/tests.rs` (or equivalent `#[path]` layout).
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-modules` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
