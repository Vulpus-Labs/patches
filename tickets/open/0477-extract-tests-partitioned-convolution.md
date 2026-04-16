---
id: "0477"
title: Extract tests from patches-dsp partitioned_convolution.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-dsp/src/partitioned_convolution.rs` is 1231 lines, of which
679 (55%) are the inline test module. Extract to a sibling
`partitioned_convolution/tests.rs`.

## Acceptance criteria

- [ ] `partitioned_convolution.rs` →
      `partitioned_convolution/mod.rs` +
      `partitioned_convolution/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-dsp` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
