---
id: "0319"
title: "patches-core: ReceivesTrackerData trait"
priority: high
created: 2026-04-11
---

## Summary

Add the `ReceivesTrackerData` opt-in trait to patches-core, following the
`ReceivesMidi` precedent. Modules that need access to pattern/song data
implement this trait and receive `Arc<TrackerData>` at plan activation.

## Acceptance criteria

- [ ] `ReceivesTrackerData` trait with method
      `fn receive_tracker_data(&mut self, data: Arc<TrackerData>)`
- [ ] `Module` trait gains default method
      `fn as_tracker_data_receiver(&mut self) -> Option<&mut dyn ReceivesTrackerData>`
      returning `None`
- [ ] The trait and method are documented with the same real-time safety
      notes as `ReceivesMidi` (no allocation, no blocking)
- [ ] `cargo test -p patches-core` passes
- [ ] `cargo clippy -p patches-core` clean

## Notes

The `Arc<TrackerData>` is cloned (ref-count bump) once per module at plan
activation. The audio thread's read path is plain pointer dereference
through the `Arc` — no atomics, no contention.

Follows exactly the `ReceivesMidi` pattern: opt-in trait, default `None`
return on `Module`, planner scans for implementors during build.

Epic: E059
ADR: 0029
