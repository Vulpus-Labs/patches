---
id: "0169"
title: Deallocate evicted plans on the cleanup thread
epic: E024
priority: medium
created: 2026-03-20
---

## Summary

In `AudioCallback::receive_plan`, the line `self.current_plan = new_plan` drops the old plan inline on the audio thread. For plans that contain `parameter_updates` with `ParameterValue::Array(Arc<[String]>)` values, this triggers an atomic refcount decrement per Array value, and potentially heap deallocation if no other reference to the Arc exists. The same cleanup-thread pattern already used for tombstoned modules (T-0052) should be applied to old plans: send the evicted plan to the `cleanup_tx` ring buffer so its destructor runs on the `"patches-cleanup"` thread.

## Acceptance criteria

- [ ] Introduce a `CleanupAction` enum (`DropModule(Box<dyn Module>)`, `DropPlan(ExecutionPlan)`) as the `cleanup_tx` element type; update all existing push/receive sites accordingly.
- [ ] `AudioCallback` sends the evicted `ExecutionPlan` via `CleanupAction::DropPlan` before replacing `self.current_plan`, rather than dropping it inline.
- [ ] If `cleanup_tx` is full (ring buffer full), fall back to inline drop with an `eprintln!` warning, matching the existing tombstone fallback pattern.
- [ ] The `HeadlessEngine` in `patches-integration-tests/src/lib.rs` is updated consistently (its `adopt_plan` mirrors the audio callback's plan-swap sequence).
- [ ] Existing off-thread deallocation integration test (`tests/off_thread_deallocation.rs`) still passes; add or extend a test that verifies an evicted plan's destructor does not run on the audio thread.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

The cleanup thread and `cleanup_tx` ring buffer were introduced in T-0052/T-0053. The ring buffer element type is currently `Box<dyn Module>`. Rather than a second ring buffer, introduce a `CleanupAction` enum as the channel element type:

```rust
pub enum CleanupAction {
    DropModule(Box<dyn Module>),
    DropPlan(ExecutionPlan),
}
```

All existing push/receive sites change from `Box<dyn Module>` to `CleanupAction::DropModule(...)`. The cleanup thread match arm just drops the inner value in both cases.

The evicted plan is the one being replaced, not the incoming one. The sequence on the audio thread becomes: pop new plan → apply tombstones/installs/updates → push `CleanupAction::DropPlan(old_plan)` → assign new plan.
