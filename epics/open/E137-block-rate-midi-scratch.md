---
id: E137
title: Block-rate MIDI scratch buffer (ADR 0069)
status: open
created: 2026-05-05
---

## Goal

Implement ADR 0069. Move MIDI event ingest from the per-sample tick
loop to a block-rate AoS scratch on `PatchProcessor`, mirroring the
host-control pattern from ADR 0068 §2 (amended 2026-05-05).

After this epic, every boundary-crossing event input (MIDI, host
control) ships through the same shape: `write_*_event(offset, ev)`
+ `prepare_*_block(frames)` + per-sample memcpy in `tick()`. The
per-sample tick loop holds no event-iteration state.

## Scope

In:

- `MidiScratch` struct on `PatchProcessor`: AoS frame
  `[MAX_BLOCK][MAX_EVENTS_PER_SAMPLE]` of `MidiEvent` plus per-sample
  count, allocated once at activation.
- New API: `processor.write_midi_event(sample_offset, event)`,
  `processor.prepare_midi_block(frames)`. Overflow within a sample
  spills into the next row.
- `tick()` per-sample MIDI flush from the AoS row into `GLOBAL_MIDI`,
  replacing the existing `write_midi` + `midi_overflow` drain.
- Player audio callback: drain the SPSC at block boundary into
  `write_midi_event`. Clamp late arrivals to `offset = 0`.
- CLAP plugin `process()`: walk `clap_input_events` once before the
  tick loop; route each MIDI event through `write_midi_event`. Same
  for `flush()` against the in/out event lists.
- Migrate integration tests / `dispatch_midi` callers / FFI harnesses
  to the new API.
- Retire `write_midi`, `midi_overflow`, and the per-sample drain in
  `tick()` once all callers have moved.

Out:

- Sample-accurate MIDI sub-block scheduling beyond what the offset
  already provides — no behaviour change at the patch level.
- Changes to `MidiFrame` layout in the backplane poly slot. The
  per-sample flush still writes a `MidiFrame` with up to
  `MidiFrame::MAX_EVENTS` events and a count; only the ingest path
  changes.
- MIDI output (engine → host); this epic is event ingest only.

## Tickets

- 0818 — `MidiScratch` on processor: AoS frame, `write_midi_event`,
  `prepare_midi_block`, per-tick flush. Existing `write_midi` stays
  alongside the new API for one ticket so callers migrate
  incrementally.
- 0819 — Migrate player + CLAP + tests to the new ingest path; retire
  `write_midi` and `midi_overflow` from `PatchProcessor`.

## Notes

- E137 sits behind ticket 0817 (host-control scratch on processor) so
  the host-control pattern is settled before MIDI follows it.
- Memory cost ~ 96 KB / instance. Within the existing audio-buffer
  budget; flag in `MidiScratch::new` rustdoc.
- Determinism: the AoS frame is a deterministic projection of the
  input event list. The hash-stability suite must pass unchanged at
  the end of 0819.
