---
id: "0474"
title: Extract tests from patches-dsp biquad.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-dsp/src/biquad.rs` is 1066 lines, of which 741 (70%) are
the inline test module. Extract to a sibling `biquad/tests.rs`.

## Acceptance criteria

- [ ] `biquad.rs` → `biquad/mod.rs` + `biquad/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-dsp` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
