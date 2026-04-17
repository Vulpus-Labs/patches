---
id: "0534"
title: Split patches-modules master_sequencer/tests.rs by category
priority: medium
created: 2026-04-17
epic: E090
superseded_by: "0546"
status: superseded
---

## Summary

[patches-modules/src/master_sequencer/tests.rs](../../patches-modules/src/master_sequencer/tests.rs)
is 774 lines. Tests cover step-shape fixtures, tick timing / swing,
transport state machine, loop-point behaviour, sync source selection
(free / host / auto), and host-sync edge behaviour.

## Superseded

Obsolete as of 2026-04-17. Ticket 0546 (follow-up to E092) migrated
the bulk of the tests in this file to
`patches-tracker-core/src/sequencer/tests.rs` — where they belong as
pure-function tests of `SequencerCore` — and reduced the module-side
file to ~215 lines covering only module-shell concerns (4 `sync_*`
parameter-resolution tests + 2 harness smoke tests for poly bus /
stop-sentinel encoding).

At six tests and ~215 lines, the file no longer warrants a category
split. The coverage the split would have organised is now organised
across two crates by concern (core logic vs module shell) rather than
by test-file line count.

## Original acceptance criteria (no longer applicable)

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

After 0546 the categories collapse: the `timing.rs`, `transport.rs`,
`loops.rs`, and `host_sync.rs` tests have moved to tracker-core and
are organised there by function (tick_duration math, transport-edge
transitions, host-sync positioning). Only `sync.rs`'s four tests
remain on the module side, plus two new harness smoke tests that
would have lived in a would-be `harness.rs` bucket.
