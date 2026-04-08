---
id: "0202"
title: Move DelayBuffer from patches-modules/common to patches-dsp
priority: medium
created: 2026-03-26
epic: E037
depends_on: "0199"
---

## Summary

Relocate `DelayBuffer`, `ThiranInterp`, `PolyDelayBuffer`, and `PolyThiranInterp`
from `patches-modules/src/common/delay_buffer.rs` into `patches-dsp`. Update all
consumers in `patches-modules` to import from the new location.

This makes `DelayBuffer` available to `patches-dsp` itself (for `PeakWindow` in
T-0203) without a circular dependency, and prepares for future migration of other
`patches-modules/common` types.

## What to do

1. Copy `patches-modules/src/common/delay_buffer.rs` verbatim into
   `patches-dsp/src/delay_buffer.rs`.
2. Re-export everything from `patches-dsp/src/lib.rs`:
   ```rust
   mod delay_buffer;
   pub use delay_buffer::{DelayBuffer, ThiranInterp, PolyDelayBuffer, PolyThiranInterp};
   ```
3. Add `patches-dsp` as a path dependency in `patches-modules/Cargo.toml`.
4. In `patches-modules/src/common/delay_buffer.rs`, replace the implementation with
   re-exports from `patches-dsp`:
   ```rust
   pub use patches_dsp::{DelayBuffer, ThiranInterp, PolyDelayBuffer, PolyThiranInterp};
   ```
   This preserves all existing `use patches_modules::common::delay_buffer::...` paths
   in module implementations without touching every call site.
5. Delete nothing else in `patches-modules/src/common/`; other files are out of scope
   for this epic.

## Acceptance criteria

- [ ] `patches_dsp::DelayBuffer` is public and passes all tests currently in
      `patches-modules/src/common/delay_buffer.rs` (move the tests to
      `patches-dsp/src/delay_buffer.rs`).
- [ ] All `patches-modules` modules that use `DelayBuffer` continue to compile without
      modification to their import paths.
- [ ] `patches-modules/src/common/delay_buffer.rs` becomes a thin re-export shim.
- [ ] `cargo test` and `cargo clippy` pass across all crates with 0 warnings.

## Notes

- Keep the re-export shim in `patches-modules` rather than updating every call site;
  that work can follow if/when `patches-modules/src/common/` is fully migrated.
- `ThiranInterp` and the poly variants should move together since they are tightly
  coupled to `DelayBuffer`.
- This ticket is independent of T-0200/T-0201 — it can be worked in parallel once
  T-0199 is done.
