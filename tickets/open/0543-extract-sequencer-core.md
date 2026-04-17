---
id: "0543"
title: Extract SequencerCore into patches-tracker-core
priority: medium
created: 2026-04-17
epic: E092
depends_on: ["0541", "0542", "0534"]
---

## Summary

Lift the pure state and logic of
[patches-modules/src/master_sequencer/mod.rs](../../patches-modules/src/master_sequencer/mod.rs)
and
[patches-modules/src/master_sequencer/playback.rs](../../patches-modules/src/master_sequencer/playback.rs)
into a `SequencerCore` struct in `patches-tracker-core`. The module
wrapper becomes port plumbing, `GLOBAL_TRANSPORT` read,
`Arc<TrackerData>` hold, `ParameterMap` validation, and poly clock-
bus encoding around a `core.tick(...)` call.

Depends on 0534 (E090's master_sequencer tests split) landing first,
so new core-level tests go into the post-split category layout
rather than the monolithic `tests.rs`.

## Acceptance criteria

- [ ] `patches-tracker-core/src/sequencer/mod.rs` defines
      `pub struct SequencerCore` holding at minimum:
      - Tempo: `bpm: f32`, `rows_per_beat: u32`
      - Swing: `swing: f32`
      - Song selection: `song_index: Option<usize>`
      - Transport state: `transport: TransportState` (moved with
        the core)
      - Loop behaviour: `do_loop: bool`
      - Position: `song_row: usize`, `pattern_step: usize`,
        `global_step: usize`
      - Sample-rate-driven timing:
        `samples_until_tick: f32`, `sample_rate: f32`
      - Edge/sentinel flags: `first_tick`, `pattern_just_reset`,
        `song_ended`, `emit_stop_sentinel`
- [ ] `SequencerCore` exposes pure methods (names indicative):
      - `new(sample_rate: f32) -> Self`
      - `set_tempo(&mut self, bpm, rows_per_beat, swing)`
      - `set_song(&mut self, song_index: Option<usize>)`
      - `reset_position(&mut self)`
      - `advance_step(&mut self, tracker: &TrackerData) -> bool`
      - `tick_duration_seconds(&self, step: usize) -> f32`
      - `tick(&mut self, tracker: &TrackerData,
              transport: TickInput, frames: usize) -> ClockBusOutput`
        where `TickInput` carries transport events (start/stop/pause/
        resume rising edges, optional host-transport frame) and
        `ClockBusOutput` carries the six clock-bus voices per frame
- [ ] `patches-modules/src/master_sequencer/mod.rs` holds only:
      `instance_id`, `descriptor`, `tracker_data: Option<Arc<…>>`,
      `core: SequencerCore`, and unavoidable port buffers.
      `playback.rs` is deleted (its contents moved to the core).
      `params.rs` and `lookup.rs` stay module-side — both touch
      `ParameterMap` and name resolution, which are module-shell
      concerns.
- [ ] `impl Module for MasterSequencer::tick()` reads transport edge
      inputs, reads `GLOBAL_TRANSPORT` if `sync == host`, calls
      `self.core.tick(...)`, encodes the `ClockBusOutput` into the
      poly clock output. No state mutation outside the core call.
- [ ] The auto-sync selection (auto picks free or host based on
      hosted flag) stays module-side; the core sees only the final
      transport input.
- [ ] `patches-tracker-core/src/sequencer/tests/` holds pure-
      function tests (categories matching 0534's split axes where
      they overlap):
      - Deterministic step advance under constant tempo
      - Swing: alternating step durations per voice
      - Loop transition: `advance_step` returns `true` across a
        loop boundary when `do_loop = true`, `false` + stop
        sentinel when `do_loop = false`
      - Stop-sentinel emission on song end with `do_loop = false`
- [ ] Module-level tests for host-sync behaviour (tests that read
      `GLOBAL_TRANSPORT`) stay in the module tests per 0534's
      layout.
- [ ] `patches-modules/src/master_sequencer/mod.rs` is under ~200
      lines after the extraction.
- [ ] `ModuleDescriptor` (ports, parameters) for `MasterSequencer`
      is byte-for-byte unchanged.
- [ ] Clock-bus voice layout unchanged (pattern reset, bank index,
      tick trigger, tick duration, step index, step fraction). The
      encoding function moves to the core; the poly-write into
      `PolyOutput` stays module-side.
- [ ] Integration tests in `patches-integration-tests/tests/tracker/`
      pass unchanged.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

**Why 0542 first.** `ClockBusFrame` is defined in 0542 on the
consumer side. 0543 produces instances of the same type on the
emitter side. Landing the consumer first means 0543 only has to
match an existing shape rather than negotiate one.

**Scheduling.** 0534 must land first. If 0534 is still open when
0543 is ready, rebase 0543 over the post-0534 test layout rather
than landing against the monolithic `tests.rs`.

**Host-transport reading.** `GLOBAL_TRANSPORT` stays a module-side
concern. The core takes a `Option<TransportFrame>` input; how that
value is sourced is not the core's business. This keeps the core
free of globals and trivially testable.

**`TrackerData` access.** Same as 0542: core takes `&TrackerData`
parameters, no Arc ownership inside. The `song_index` name lookup
stays in `params.rs` / `lookup.rs` — the core receives an already-
resolved `Option<usize>`.
