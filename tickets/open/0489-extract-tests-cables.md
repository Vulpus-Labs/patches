---
id: "0489"
title: Extract tests from patches-core cables.rs
priority: low
created: 2026-04-16
---

## Summary

`patches-core/src/cables.rs` is 824 lines, of which 270 (33%) are
the inline test module. Extract to a sibling `cables/tests.rs`.

## Acceptance criteria

- [ ] `cables.rs` → `cables/mod.rs` + `cables/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-core` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change. `cables.rs`
contains multiple port-type structs (Mono/Poly × Input/Output,
Trigger, Gate); further split into one file per port-type family
is tracked in follow-on epic (tier B9).
