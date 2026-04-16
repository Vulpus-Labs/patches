---
id: "0478"
title: Extract tests from patches-dsp svf.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-dsp/src/svf.rs` is 968 lines, of which 664 (69%) are the
inline test module. Extract to a sibling `svf/tests.rs`.

## Acceptance criteria

- [ ] `svf.rs` → `svf/mod.rs` + `svf/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-dsp` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
