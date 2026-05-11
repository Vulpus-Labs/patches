---
id: "0861"
title: Migrate `with_cycle_only` callers; delete the constructor
priority: medium
created: 2026-05-10
epic: E143
depends-on: "0860"
---

## Summary

`CablePool::with_cycle_only` was kept after 0850 to spare ~15 test
callsites from threading a scratch slice. Under the post-0858 fused-read
rules — and even more so under the 0860 fused-true-by-default
invariant — it is a foot-gun: any test that ticks a module reading a
backplane slot (or any module flipped to scratch via fusion analysis)
hits a debug-assert bounds error from the empty scratch slice.

Migrate every caller to `CablePool::new` with an explicit scratch
slice, then delete `with_cycle_only`.

## Acceptance criteria

- [x] Each caller of `CablePool::with_cycle_only` passes an explicit
      scratch slice sized at least `RESERVED_SLOTS` (so all backplane
      reads are in-bounds) or appropriately for the test's needs.
      Affected sites:
  - `patches-ffi/tests/gain_plugin.rs` (2)
  - `patches-integration-tests/tests/ffi_alloc_trap_cycles.rs` (2)
  - `patches-core/src/cables/tests.rs` (~10)
  - `patches-core/src/cables/mod.rs` (doc reference)
  - `patches-modules/examples/filter_bench.rs`
  - `patches-modules/src/pattern_player/tests.rs` (2)
  - `patches-engine/src/execution_state.rs` (test-only)
- [x] `CablePool::with_cycle_only` removed from
      `patches-core/src/cable_pool.rs`. Doc comment on `CablePool::new`
      no longer references the legacy constructor.
- [x] A small test helper (`patches-core::test_support::cable_pool::
      empty_scratch()` or similar) may be introduced if the migration
      reveals a common shape across multiple crates. Optional — only
      add it if more than one consumer pattern recurs.
- [x] `just push` clean.

## Notes

Under 0860 the dispatch cutoff is `SCRATCH_CAPACITY`, and disconnected
ports default to `fused: true` with `cable_idx = MONO_READ_SINK = 0`.
A test that hands `CablePool::new(&mut [], …)` an empty scratch will
hit the bounds check immediately on the first disconnected read. The
test helper must always supply at least `RESERVED_SLOTS` of zero-init
scratch — the natural place for that is in `test_support`.
