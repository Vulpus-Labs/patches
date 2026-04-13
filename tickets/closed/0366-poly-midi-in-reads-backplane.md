---
id: "0366"
title: PolyMidiIn reads MIDI from backplane poly input
priority: medium
created: 2026-04-12
---

## Summary

Refactor `PolyMidiIn` to read MIDI events from the `GLOBAL_MIDI`
backplane slot (via `MidiFrame`) instead of implementing the
`ReceivesMidi` trait. This makes it a pure graph module with no
special engine-level dispatch, and allows it to sit at the end of
a MIDI transform chain.

## Acceptance criteria

- [ ] `PolyMidiIn` gains a fixed poly input wired to `GLOBAL_MIDI`
      (same pattern as `HostTransport` reading `GLOBAL_TRANSPORT`)
- [ ] Each tick, reads event count and events from the poly input
      using `MidiFrame` accessors
- [ ] Feeds decoded events into existing voice allocation logic
      (voct, gate, trigger, velocity outputs unchanged)
- [ ] `ReceivesMidi` implementation removed from `PolyMidiIn`
- [ ] Existing integration tests and DSL patches using
      `poly_midi_in` continue to work
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- The fixed backplane input means `poly_midi_in` works without
  explicit wiring in the patch, just as it does today.
- Later, when MIDI transform modules exist, the input can be
  overridden by an explicit connection to a transform module's
  MIDI poly output instead of the backplane.
- Other `ReceivesMidi` implementors (`MidiIn`, `MidiCc`,
  `MidiDrumset`) can be migrated in follow-up tickets.
