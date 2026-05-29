---
id: "0990"
title: Builder owns tracker-receiver detection via InstallMeta
priority: low
created: 2026-05-29
---

## Summary

`Planner::build_full` currently mutates `ExecutionPlan` **after** the builder
returns:

```rust
for (_, m) in plan.new_modules.iter_mut() {
    if m.as_tracker_data_receiver().is_some() {
        new_tracker_ids.insert(m.instance_id());
    }
}
// ...
plan.tracker_receiver_indices = ...;
plan.tracker_data = tracker_data.map(Arc::new);
```

This is the only site that mutates `ExecutionPlan` post-build, and the only
reason it exists is that `InstallMeta` doesn't carry the tracker-receiver
capability. Detecting it at install time inside the builder folds the logic
into the typed pipeline.

Plan:

1. Add `is_tracker_receiver: bool` to `InstallMeta` (set by `instantiate` from
   `Module::as_tracker_data_receiver().is_some()` while the module is in
   hand).
2. The builder maintains the surviving-receiver set and emits
   `tracker_receiver_indices` directly into `ExecutionPlan` during
   `build_draft` / `assemble`.
3. `Planner::build_full` shrinks to: pass `tracker_data` into the builder
   (new parameter on `build_patch_with_meta` and friends), no
   post-build mutation.

## Acceptance criteria

- [ ] `InstallMeta` carries `is_tracker_receiver: bool`; populated inside
      `instantiate` from the in-hand module.
- [ ] The builder's surviving-receiver bookkeeping moves from `Planner` into
      `PatchBuilder` (it's already a function of `prev_state.module_alloc.pool_map`
      + the install-time capability flags).
- [ ] `ExecutionPlan::tracker_receiver_indices` is populated by the builder;
      `Planner::build_full` does not write it.
- [ ] `ExecutionPlan::tracker_data` is set inside the builder, taking
      `tracker_data: Option<TrackerData>` as a builder argument; `Planner`
      passes the value through, does not assign post-hoc.
- [ ] `Planner` still owns the cross-build receiver instance-id set
      (`HashSet<InstanceId>`) used to track survivors, or that state moves
      into `PlannerState` — whichever keeps the `Planner` thin wrapper
      semantically a wrapper.
- [ ] Audio goldens bit-identical; tracker-receiver integration tests green.

## Notes

Part of epic **E162**. Depends on 0989 (positional `InstallMeta`). Cleans up
the only post-build mutation in `Planner::build_full`, restoring the
invariant that `ExecutionPlan` returned by the builder is the final plan.
Low priority: the current code works correctly; this is structural.
