---
id: "0322"
title: "ExecutionPlan: tracker_data field and receiver indices"
priority: high
created: 2026-04-11
---

## Summary

Extend `ExecutionPlan` in patches-engine to carry `Arc<TrackerData>` and
a list of module pool indices that implement `ReceivesTrackerData`.

## Acceptance criteria

- [ ] `ExecutionPlan` gains `tracker_data: Option<Arc<TrackerData>>` field
- [ ] `ExecutionPlan` gains `tracker_receiver_indices: Vec<usize>` field
- [ ] `PatchBuilder` populates `tracker_receiver_indices` by scanning
      modules via `Module::as_tracker_data_receiver()`
- [ ] `PatchBuilder` accepts `TrackerData` from the interpreter and wraps
      it in `Arc`
- [ ] Existing plan adoption/activation code handles the new fields
      (no-op if `tracker_data` is `None`)
- [ ] `cargo test -p patches-engine` passes
- [ ] `cargo clippy -p patches-engine` clean

## Notes

Follows the `midi_receiver_indices` pattern exactly. The `tracker_data`
is `None` for patches that don't use pattern/song blocks — zero overhead
for non-tracker patches.

Epic: E059
ADR: 0029
