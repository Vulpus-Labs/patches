---
id: "0817"
title: Move host-control scratch + pipeline from HostControl module to PatchProcessor
priority: high
created: 2026-05-05
epic: E135
depends_on: "0809,0811,0816"
supersedes: "0816"
---

## Summary

Relocate the host-control per-block pipeline (SoA scratch, smoothing,
AoS transpose, per-sample memcpy to the backplane region) from the
`HostControl` module to `PatchProcessor`, per the ADR 0068
amendment 2026-05-05.

The `HostControl` module's runtime contract becomes "read the
backplane lane, write the output port" — a trivial demux. All the
per-block machinery moves to the engine, where it sits beside the
existing transport / MIDI flush surface and is reachable from CLAP
`process()` via a clean processor-level API instead of a `module_pool`
lookup.

This unblocks the CLAP audio-thread parameter event queue (last
deferred piece of ticket 0811): wiring is just
`processor.write_host_control_event(...)` + `prepare_host_control_block(frames)`,
the same shape as `write_midi`.

## Acceptance criteria

### Engine surface

- [ ] `PatchProcessor` owns `HostControlScratch`:
      - SoA scratch `[MAX_HOST_CONTROLS][MAX_HOST_CONTROL_BLOCK]`,
      - AoS frame `[MAX_HOST_CONTROL_BLOCK][MAX_HOST_CONTROLS]`,
      - per-lane tail state `[f32; MAX_HOST_CONTROLS]`,
      - per-lane `kind` table `[HostControlKind; MAX_HOST_CONTROLS]`,
      - smoothing α (computed from sample rate × ~5 ms τ).
      Allocated once at activation. No allocation on the audio thread.
- [ ] `processor.write_host_control_event(channel, sample_offset, value)`
      — analogue of `write_midi`. Pushes one event onto a sorted
      per-block buffer; events with `channel >= MAX_HOST_CONTROLS` or
      `sample_offset >= block_size` are dropped silently.
- [ ] `processor.prepare_host_control_block(frames)` runs the
      step-fill / smooth / transpose pipeline against the buffered
      events and resets the per-tick `sample_idx`. Idempotent within
      a block; safe to call with an empty event buffer (all rows
      carry forward).
- [ ] In `tick()`, before module dispatch, the processor memcpys the
      AoS row at `sample_idx` into the four contiguous
      `HOST_CONTROL_BASE..HOST_CONTROL_BASE + HOST_CONTROL_SLOTS` poly
      slots — one 256-byte run, no per-channel loop, same shape as
      the existing transport / MIDI flush. `sample_idx` advances each
      tick; underrun (no `prepare_host_control_block` for this buffer)
      writes zeros to the backplane region.

### Plan-adoption surface

- [ ] `PlanMeta` carries:
      - `(ParamId → channel)` map for resolving incoming
        `clap_event_param_value::param_id` to a scratch row,
      - per-channel `lane_kind` table so the scratch knows which rows
        skip smoothing.
      Both are populated by the planner from the host-control manifest.
- [ ] `PatchProcessor::adopt_plan_with_meta` installs the new tables
      onto the scratch atomically with the plan swap. Until the first
      manifest, both tables are empty / all-Knob defaults.

### Module shrinkage

- [ ] `patches-modules/src/host_control.rs` is reduced to a backplane
      reader: per-channel `slot_offset` + `kind`; `process()` reads
      `pool.read_poly(HOST_CONTROL_BASE + slot_offset/16)[slot_offset%16]`
      and writes the value to `audio_out[i]` (knob/slider/toggle) or
      `trigger_out[i]` (trigger).
- [ ] Drop from `HostControl`: `scratch_soa`, `frame_aos`, `tail`,
      `lane_kinds`, `smooth_alpha`, `block_size`, `sample_idx`,
      `prepare_block`, `set_lane_kinds`, `frame_row`, `tail` accessor.
      Drop the `HostControlEvent` re-export from `patches-modules`;
      it now lives in `patches-engine` (or `patches-core`, see
      Notes).

### CLAP integration

- [ ] `patches-clap` `process()`:
      - Walks the `clap_input_events` queue; for each
        `CLAP_EVENT_PARAM_VALUE` resolves `param_id → channel` against
        the per-plan map (cached on the audio side at adoption), calls
        `processor.write_host_control_event(...)`.
      - Calls `processor.prepare_host_control_block(frames)` once
        before the per-sample tick loop.
      - `params_flush` does the same against the in/out event lists.
- [ ] `params_get_value` continues to read the registry's
      `last_value`. The audio thread reports back through the existing
      param-update channel (or, if simpler, through a per-id atomic
      surface analogous to `LatestValues`) — design call inside this
      ticket.

### Test relocation

- [ ] Pipeline tests (step-fill / smoothing / transpose / trigger /
      kind dispatch / end-to-end memcpy) move from
      `patches-modules/src/host_control.rs` to a new
      `patches-engine` module next to `HostControlScratch`.
- [ ] `HostControl` module tests collapse to: per-kind output port
      receives the backplane lane value; out-of-range `slot_offset`
      degrades to 0.0.
- [ ] New integration test: feed a synthetic event list through
      `processor.write_host_control_event` + `prepare_host_control_block`,
      tick the engine, assert `audio_out[i]` reflects the smoothed
      value at each sample.

### ADR / docs

- [ ] ADR 0068 §2 amendment (already landed 2026-05-05) cited from
      the new code's module docs.
- [ ] ADR 0057 §4: update the "host events feed an SoA scratch ..."
      paragraph to point at `PatchProcessor`, not the `HostControl`
      module.

### Validation

- [ ] `just inner -p patches-engine -p patches-modules -p patches-clap`
      passes.
- [ ] `just commit -p patches-engine -p patches-modules -p patches-clap`
      passes (clippy clean on touched scope).
- [ ] `just push` passes including determinism / hash-stability suite
      (smoothing must be bit-deterministic across runs at the same
      sample rate).

## Notes

- `HostControlEvent` is shaped like `MidiEvent` and is processor-level
  state; it belongs in `patches-engine` next to `HostControlScratch`,
  or in `patches-core` if we want non-engine crates (e.g. tests) to
  construct events without pulling the engine. Pick the lighter
  dependency edge.
- The per-id audio→main reporting surface for `last_value`: simplest
  shape is a `[AtomicU32; MAX_HOST_CONTROLS]` (bit-cast f32) on the
  scratch, stamped per `prepare_host_control_block`. Main thread
  drains via `record_value(id, scratch[channel].load())` once per
  `on_main_thread`. Trade-off vs. an SPSC ring is per-knob granularity
  — atomics give "current value", a ring gives every event.
- This ticket does not change ADR 0057 §6 (registry / tombstone /
  cookie semantics) — those are intact from ticket 0811.
- Default fall-through behaviour: if the planner ships an empty
  `(ParamId → channel)` map (patch declares no host controls), the
  scratch's per-tick memcpy still runs and writes the zero-initialised
  AoS row to the backplane region, leaving the four slots quiet.
- The `HostControl` module remains in the registry with the same
  descriptor template; only its body changes. Existing patches keep
  compiling without churn.
