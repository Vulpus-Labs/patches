---
id: "0379"
title: Add MIDI backplane helpers and shared test utilities
priority: medium
created: 2026-04-13
---

## Summary

All four MIDI modules (`MonoMidiIn`, `PolyMidiIn`, `MidiCc`, `MidiDrumset`)
repeat identical boilerplate: a 4-field `PolyInput` construction in `prepare()`
and a 4-line frame-reading loop in `process()`. Their tests also duplicate
`send_midi`, `note_on`, `note_off`, and `cc` helpers.

## Acceptance criteria

- [ ] Add `PolyInput::backplane(cable_idx: usize) -> PolyInput` constructor to `patches-core`
- [ ] Add `MidiFrame::iter_events(frame: &[f32; 16]) -> impl Iterator<Item = MidiEvent>` to `patches-core`
- [ ] All four MIDI modules use the new constructor and iterator
- [ ] Add shared `send_midi`, `note_on`, `note_off`, `cc` test helpers to `patches-core::test_support`
- [ ] All four MIDI module test suites use the shared helpers
- [ ] No behaviour changes — all existing tests pass

## Notes

The `iter_events` iterator should yield `MidiEvent` values (with the full
status byte — channel stripping remains the caller's responsibility). The
`PolyInput::backplane` constructor sets `scale: 1.0, connected: true`.
