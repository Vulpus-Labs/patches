---
id: "0527"
title: Split patches-modules filter/mod.rs by variant
priority: low
created: 2026-04-17
epic: E090
---

## Summary

[patches-modules/src/filter/mod.rs](../../patches-modules/src/filter/mod.rs)
is 587 lines with three near-parallel filter variants:
`ResonantLowpass`, `ResonantHighpass`, `ResonantBandpass`. Each has its
own struct, `new`, `Module` impl, and `PeriodicUpdate` impl. Plus a
shared `resonance_to_q` helper at the top and a sibling `tests.rs`.

## Acceptance criteria

- [ ] Convert `filter/mod.rs` to retain shared helpers + module
      declarations, with new sibling submodules:
      `lowpass.rs` (ResonantLowpass + impls),
      `highpass.rs` (ResonantHighpass + impls),
      `bandpass.rs` (ResonantBandpass + impls).
- [ ] `resonance_to_q` stays in `mod.rs` (or moves to a `common.rs`
      sibling) so each variant can reach it via `super::`.
- [ ] Registrations (`FILTER_TYPE_*` constants and any module-type
      registry entries) and re-exports unchanged.
- [ ] `mod.rs` under ~150 lines.
- [ ] `cargo build -p patches-modules`, `cargo test -p patches-modules`,
      `cargo clippy` clean.

## Notes

E090. Pattern: see `mixer/` after 0506. Audio-thread invariants
preserved.
