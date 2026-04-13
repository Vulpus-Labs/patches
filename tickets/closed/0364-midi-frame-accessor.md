---
id: "0364"
title: MidiFrame accessor struct
priority: medium
created: 2026-04-12
---

## Summary

Introduce a `MidiFrame` struct in `patches-core` that provides
named accessors for packing and unpacking MIDI events into an
`[f32; 16]` poly frame. Lane 0 carries an event count; lanes 1–15
carry up to 5 MIDI events as (status, data1, data2) triples.

## Acceptance criteria

- [ ] `MidiFrame` struct in `patches-core` with constants for
      `EVENT_COUNT` (lane 0) and `MAX_EVENTS` (5)
- [ ] `event_count(&[f32; 16]) -> usize` reader
- [ ] `read_event(&[f32; 16], index) -> MidiEvent` reader
- [ ] `write_event(&mut [f32; 16], index, MidiEvent)` writer
- [ ] `set_event_count(&mut [f32; 16], usize)` writer
- [ ] `clear(&mut [f32; 16])` to reset frame to zero events
- [ ] Unit tests covering round-trip encode/decode of 0, 1, and
      5 events
- [ ] Unit test that all event slots fit within 16 lanes
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- See ADR 0033 for design rationale.
- 5 events × 3 lanes = 15 lanes + 1 count lane = 16 total. This
  fills the poly frame exactly.
- f32 faithfully represents all u8 values (0–255), so the
  encoding is lossless for MIDI bytes.
- This ticket defines the frame format only — wiring it into the
  backplane and modules comes in later tickets.
