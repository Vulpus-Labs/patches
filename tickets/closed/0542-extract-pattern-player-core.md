---
id: "0542"
title: Extract PatternPlayerCore into patches-tracker-core
priority: medium
created: 2026-04-17
epic: E092
---

## Summary

Lift the pure state and logic of
[patches-modules/src/pattern_player/mod.rs](../../patches-modules/src/pattern_player/mod.rs)
into a `PatternPlayerCore` struct in `patches-tracker-core`. The
module wrapper becomes port decode + `core.tick(...)` + port encode.
The existing tests in
[patches-modules/src/pattern_player/tests.rs](../../patches-modules/src/pattern_player/tests.rs)
already reach into fields that would become core state
(`apply_step`, `prev_tick_trigger`, `trigger_pending`, slide state),
so this is largely field redirection rather than a rewrite.

PatternPlayer is the smaller of the two extractions and proves the
pattern before 0543 tackles MasterSequencer.

## Acceptance criteria

- [ ] `patches-tracker-core/src/pattern_player/mod.rs` defines
      `pub struct PatternPlayerCore` holding at minimum:
      - Per-channel step-index state
      - Per-channel `cv1`, `cv2`, `gate`, `trigger_pending` values
      - Slide state (current target, ramp progress)
      - `prev_tick_trigger` edge-detect memory
      - `sample_rate` and `channels`
- [ ] `PatternPlayerCore` exposes at least these pure methods
      (names are guidance, not prescriptive):
      - `new(sample_rate: f32, channels: usize) -> Self`
      - `apply_step(&mut self, channel: usize, step: &PatternStep)`
        — pure state transition for one step event
      - `tick(&mut self, clock_bus: &ClockBusFrame,
              tracker: &TrackerData, bank_index: i32) -> TickOutputs`
        where `TickOutputs` holds per-channel cv1/cv2/trigger/gate
- [ ] `patches-modules/src/pattern_player/mod.rs` holds only:
      `instance_id`, `descriptor`, `tracker_data: Option<Arc<…>>`,
      and `core: PatternPlayerCore`. All state-mutation methods
      have been removed from the `PatternPlayer` impl.
- [ ] `impl Module for PatternPlayer::tick()` reads the poly clock
      input, constructs a `ClockBusFrame` value, calls
      `self.core.tick(&frame, &tracker, bank)`, and writes
      `TickOutputs` to the port buffers. No state mutation outside
      the core call.
- [ ] Existing tests in
      `patches-modules/src/pattern_player/tests.rs` pass unchanged,
      retargeted to call `core.apply_step(...)` etc. where they
      previously called `self.apply_step(...)`. The test count
      does not drop.
- [ ] A sibling `patches-tracker-core/src/pattern_player/tests.rs`
      holds at least three pure-function tests:
      - `apply_step` for a note event produces correct cv1/cv2/gate
      - `tick` with no trigger leaves outputs at the previous step's
        values
      - Trigger edge detect fires exactly once per clock-bus
        trigger rising edge
- [ ] `patches-modules/src/pattern_player/mod.rs` is under ~200 lines
      after the extraction.
- [ ] Integration tests in `patches-integration-tests/tests/tracker/`
      pass unchanged.
- [ ] `ModuleDescriptor` (ports, parameters) for `PatternPlayer` is
      byte-for-byte unchanged.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

Three design points worth pinning down in the PR, not in this ticket:

1. **`ClockBusFrame` type location.** The struct decoding the six
   clock-bus voices (pattern reset, bank index, tick trigger, tick
   duration, step index, step fraction) should live in
   `patches-tracker-core`, since `SequencerCore` (0543) will emit
   it and `PatternPlayerCore` will consume it. Defining it here
   avoids retrofitting in 0543.
2. **`TrackerData` access.** The core takes `&TrackerData` as a
   parameter rather than holding an `Arc`. Arc ownership stays on
   the module wrapper. This keeps the core trivially testable
   without constructing Arcs in test fixtures.
3. **Poly channel layout.** The core's `tick()` operates over all
   channels; it does not assume any particular poly-port encoding.
   The module wrapper handles `InputPort`/`OutputPort` specifics.

No ADR needed beyond 0042 (landed in 0541).
