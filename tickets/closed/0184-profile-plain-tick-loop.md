---
id: "0184"
title: Wire TimingShim into profile.rs; plain tick loop; collector report
priority: medium
created: 2026-03-24
epic: E033
depends: "0183"
---

## Summary

Update `patches-profiling/src/bin/profile.rs` to wrap every module in
`plan.new_modules` with a `TimingShim` after `build_patch` returns, then run
the patch through a plain `plan.tick()` loop for `PROFILE_ITERS` ticks (no
per-slot timing inline), and finally call `collector.report()` to print results.
The `time_all_slots` function introduced in the previous fix is removed.

## Acceptance criteria

- [ ] After `build_patch`, `plan.new_modules` is drained and each `Box<dyn Module>` is replaced with `Box::new(TimingShim::new(m, Arc::clone(&collector)))`
- [ ] The profiling loop calls `plan.tick(pool, &mut cable_pool)` with no inline timing — all measurement happens inside the shims
- [ ] `periodic_update` calls are made automatically via `plan.tick`'s `periodic_indices` path; no manual handling needed
- [ ] `collector.report()` is called after the loop and results are printed in two sections:
  - **Per-type summary**: module name, instance count, avg process ns/call, avg periodic ns/call (if any), combined total ns, % of total
  - **Per-instance detail**: one row per `(module_name, instance_id)`, same columns, sorted by total time descending
- [ ] The overall headroom estimate accounts for both `process` and `periodic_update` contributions; periodic cost is amortised as `periodic_total_ns / PROFILE_ITERS` per sample (since `periodic_update` fires every `COEFF_UPDATE_INTERVAL` samples, its total ns is already the correct amortised sum across all ticks)
- [ ] `cargo clippy -p patches-profiling` clean; `cargo test` passes

## Notes

The drain-and-wrap pattern:
```rust
plan.new_modules = plan.new_modules.drain(..)
    .map(|(idx, m)| (idx, Box::new(TimingShim::new(m, Arc::clone(&collector))) as Box<dyn Module>))
    .collect();
```

The `pool_index_names` helper and the per-type aggregation from the current
`profile.rs` should be reused for the summary table. The per-instance section
is new and uses `TimingRecord::instance_id` from the collector.
