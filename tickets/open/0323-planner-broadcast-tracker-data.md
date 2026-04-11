---
id: "0323"
title: "Planner: broadcast Arc<TrackerData> on plan activation"
priority: high
created: 2026-04-11
---

## Summary

Wire the plan activation path to call `receive_tracker_data` on each
module in the `tracker_receiver_indices` list, passing a clone of the
`Arc<TrackerData>`.

## Acceptance criteria

- [ ] On plan adoption, iterate `tracker_receiver_indices` and call
      `receive_tracker_data(arc.clone())` on each module via the module
      pool
- [ ] `Arc::clone` is a ref-count bump only — no allocation on the audio
      thread
- [ ] On hot-reload, the new plan carries a new `Arc<TrackerData>`; the
      old plan's `Arc` is sent to the cleanup thread for deallocation
- [ ] Patches without tracker data skip the broadcast entirely
- [ ] Unit or integration test: build a plan with a mock
      `ReceivesTrackerData` module, verify it receives the data
- [ ] `cargo test -p patches-engine` passes
- [ ] `cargo clippy -p patches-engine` clean

## Notes

The broadcast happens during plan adoption, which runs on the audio
thread at a sub-block boundary. The only work is iterating a `Vec<usize>`
and calling a method that stores an `Arc` — this is O(n) in the number of
tracker-receiving modules (typically 2–5) with no allocation.

Epic: E059
ADR: 0029
