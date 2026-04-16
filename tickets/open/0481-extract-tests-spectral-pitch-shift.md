---
id: "0481"
title: Extract tests from patches-dsp spectral_pitch_shift.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-dsp/src/spectral_pitch_shift.rs` is 885 lines, of which
404 (46%) are the inline test module. Extract to a sibling
`spectral_pitch_shift/tests.rs`.

## Acceptance criteria

- [ ] `spectral_pitch_shift.rs` →
      `spectral_pitch_shift/mod.rs` +
      `spectral_pitch_shift/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-dsp` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change.
