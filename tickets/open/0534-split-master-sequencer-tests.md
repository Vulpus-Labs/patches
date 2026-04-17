---
id: "0534"
title: Split patches-modules master_sequencer/tests.rs by category
priority: medium
created: 2026-04-17
epic: E090
---

## Summary

[patches-modules/src/master_sequencer/tests.rs](../../patches-modules/src/master_sequencer/tests.rs)
is 774 lines. Tests cover step-shape fixtures, tick timing / swing,
transport state machine, loop-point behaviour, sync source selection
(free / host / auto), and host-sync edge behaviour.

## Acceptance criteria

- [ ] Convert to stub `src/master_sequencer/tests.rs` declaring a
      submodule tree under `src/master_sequencer/tests/`.
- [ ] Category split (final naming the ticket's call):
      - `timing.rs` — `tick_timing_*`, `swing_tick_*`
      - `transport.rs` — `transport_state_machine`
      - `loops.rs` — `loop_point_behaviour`, `end_of_song_no_loop`
      - `sync.rs` — `sync_auto_*`, `sync_free_*`, `sync_host_*`
      - `host_sync.rs` — `host_sync_*` edge tests
- [ ] Shared fixtures (`shape`, `simple_step`, `make_sequencer`) in
      `tests/mod.rs` or `tests/support.rs`.
- [ ] `cargo test -p patches-modules` passes with the same test
      count.
- [ ] `cargo build -p patches-modules`, `cargo clippy` clean.

## Notes

E090. No test logic edits.
