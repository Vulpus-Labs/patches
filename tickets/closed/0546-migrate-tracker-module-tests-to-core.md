---
id: "0546"
title: Migrate tracker module tests to tracker-core
priority: medium
created: 2026-04-17
follows: E092
---

## Summary

E092 extracted `SequencerCore` and `PatternPlayerCore` into
`patches-tracker-core` but left most of the pre-existing tests inside
the `patches-modules` shells. Many of those tests reach directly into
core fields (`seq.core.song_row`, `seq.core.pattern_step`,
`seq.core.advance_step(&data)`, `player.core.apply_step(...)`) and are
really core-logic tests wearing a `ModuleHarness` costume. Move them
to `patches-tracker-core` where they belong and where they can run
without building a module harness.

Keep at the module level only the thin slice that genuinely tests
module-shell behaviour: poly-bus encoding, `CablePool` port read/write,
`ParameterMap` → core-field bridging, and `GLOBAL_TRANSPORT` wiring.

## Acceptance criteria

### `PatternPlayer` tests

- [ ] `patches-tracker-core/src/pattern_player/tests.rs` gains:
      - A test reproducing `basic_step_playback` (note, tie step with
        `trigger=false gate=true`, rest) driving `apply_step` directly.
      - A test reproducing `tie_holds_gate` semantics.
      - A `slide_interpolation` test exercising `apply_step` with
        `cv1_end`/`cv2_end` and checking the interpolation ramp.
      - A `repeat_subdivision` test checking `repeat_active`,
        `repeat_count`, `repeat_interval_samples`, initial
        `repeat_index` after `apply_step`.
      - A `channel_count_mismatch_excess_ignored` test (pattern has
        more channels than the player).
      - A `channel_count_mismatch_surplus_silent` test (player has more
        channels than the pattern).
- [ ] `patches-modules/src/pattern_player/tests.rs` is reduced to:
      - The existing `repeat_via_process_produces_triggers_and_gate_cycles`
        end-to-end test (it exercises `CablePool` + poly clock decode
        + mono output write — the module shell's job).
      - All field-poking tests (`basic_step_playback`, `tie_holds_gate`,
        `slide_interpolation`, `repeat_subdivision`, `stop_sentinel_clears_all`,
        `channel_count_mismatch_*`) are removed (now covered in core).
- [ ] Core-level test count for `PatternPlayerCore` grows from 4 to at
      least 9.

### `MasterSequencer` tests

- [ ] `patches-tracker-core/src/sequencer/tests.rs` gains:
      - A `tick_duration_math` test covering `tick_duration_seconds`
        and `tick_duration_samples` at 120 BPM / 4 rows-per-beat.
      - A `transport_edge_state_transitions` test driving `tick_free`
        with synthetic `TransportEdges` rising edges for
        start → pause → resume → stop, verifying `core.state`.
      - A `host_sync_first_tick_fires_on_playing_edge` test driving
        `tick_host` across a 0→1 transition in `HostTransport.playing`.
      - A `host_sync_freezes_on_stop` test verifying that a 1→0
        transition sets `state == Paused` without resetting position.
      - A `host_sync_mid_song_start` test (start at beat 8.0 with
        4-row song → expect `song_row == 2`, `pattern_step == 0`,
        tick_fired and reset_fired set on the first tick).
      - A `host_sync_mid_bar_start` test (start mid-bar, expect
        correct `pattern_step` and non-zero `step_fraction`).
      - A `host_sync_three_four_time` test driving `tick_host` with
        `tsig_num=3, tsig_denom=4` and checking step mapping.
      - A `host_sync_loop_wrapping` test verifying `resolve_song_row`
        wraps past song end to `loop_point`.
      - A `host_sync_non_looping_end` test verifying that
        `resolve_song_row` past end with `do_loop=false` sets
        `song_ended` and `emit_stop_sentinel`.
      - A `host_sync_daw_seek` test verifying position jumps when
        `HostTransport.beat` jumps.
- [ ] `patches-modules/src/master_sequencer/tests.rs` is reduced to:
      - The four `sync_*` tests (`sync_auto_selects_host_when_hosted`,
        `sync_auto_selects_free_when_standalone`,
        `sync_free_overrides_hosted`, `sync_host_overrides_standalone`)
        — these test `ParameterMap` → `use_host_transport` resolution,
        which is a module-shell concern.
      - A single end-to-end `host_sync_poly_bus_encoding` smoke test
        that drives a `ModuleHarness` with `GLOBAL_TRANSPORT` lanes
        set and asserts the poly clock bus voices (0..5) match the
        expected values on tick-fire.
      - A single end-to-end `stop_sentinel_poly_encoding` smoke test
        that forces `emit_stop_sentinel` and reads the poly bus,
        checking bank index −1 and tick-trigger 1 on all channels.
      - Everything else (`tick_timing_*`, `swing_tick_durations`,
        `transport_state_machine`, `loop_point_behaviour`,
        `end_of_song_no_loop`, all other `host_sync_*`) removed
        (now covered in core).
- [ ] Core-level test count for `SequencerCore` grows from 4 to at
      least 14.
- [ ] Total `MasterSequencer` module test count drops from 17 to ~6.

### Cross-cutting

- [ ] `cargo build`, `cargo test --workspace`, `cargo clippy --workspace`
      clean.
- [ ] `patches-integration-tests/tests/tracker/` passes unchanged.
- [ ] Net test count (core + module) does not drop — every migrated
      test lands somewhere, and the four or so redundant tests are
      removed only if already covered.

## Notes

### Why not do this inside E092

The epic's scope was the extraction itself: moving state + logic,
leaving existing tests functionally equivalent. Test-layout
reorganisation is a separate concern and wants its own reviewer pass
(is the migration faithful? are the module-side survivors the right
shape?).

### Coordination with 0534

0534 (master_sequencer tests category split) is still open. If 0534
lands before this ticket, the six survivors go into the post-split
category layout:

- `sync.rs` → the 4 `sync_*` tests stay.
- `host_sync.rs` → the poly-bus encoding smoke test goes here.
- `transport.rs` → the stop-sentinel poly-encoding smoke test goes
  here.
- `timing.rs`, `loops.rs` → become empty and are deleted or
  merged away by this ticket.

If this ticket lands first, 0534's split becomes unnecessary (the
file will be ~5 tests, not 774 lines). Update 0534 with a note or
close it as superseded.

### Module-side survivor shape

The six surviving module tests should be the minimum coverage that
would catch a bug in the module shell itself — poly encoding, port
wiring, parameter bridging, `GLOBAL_TRANSPORT` reading. If any of
these tests could be rewritten to run against the core alone, that's
a sign it should migrate.

### Harness smoke tests vs core tests

The two surviving harness tests (`host_sync_poly_bus_encoding`,
`stop_sentinel_poly_encoding`) are deliberately thin: one tick with
a known input, one read of the poly output, a handful of
assertions on the bus voices. They are not about sequencer logic —
the sequencer logic has already been tested at the core level.
They are about verifying that the module wrapper correctly decodes
`GLOBAL_TRANSPORT`, calls the core, and encodes the result into
the poly bus.
