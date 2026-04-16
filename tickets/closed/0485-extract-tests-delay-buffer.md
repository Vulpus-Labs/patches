---
id: "0485"
title: Extract tests from patches-dsp delay_buffer.rs
priority: low
created: 2026-04-16
---

## Summary

`patches-dsp/src/delay_buffer.rs` is 655 lines, of which 324 (49%)
are the inline test module. Extract to a sibling
`delay_buffer/tests.rs`.

## Acceptance criteria

- [ ] `delay_buffer.rs` → `delay_buffer/mod.rs` +
      `delay_buffer/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-dsp` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
