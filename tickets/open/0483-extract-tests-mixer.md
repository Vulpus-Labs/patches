---
id: "0483"
title: Extract tests from patches-modules mixer.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-modules/src/mixer.rs` is 1031 lines, of which 371 (36%)
are the inline test module. Extract to a sibling `mixer/tests.rs`.

## Acceptance criteria

- [ ] `mixer.rs` → `mixer/mod.rs` + `mixer/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-modules` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change. `mixer.rs`
contains four distinct mixer types (Mixer, StereoMixer, PolyMixer,
StereoPolyMixer); further split into one file per type is tracked
in follow-on epic (tier B8).
