---
id: "0369"
title: MonoMidiIn reads MIDI from backplane
priority: medium
created: 2026-04-13
---

## Summary

Refactor `MonoMidiIn` to read MIDI events from the `GLOBAL_MIDI`
backplane slot (via `MidiFrame`) instead of implementing the
`ReceivesMidi` trait, following the same pattern as `PolyMidiIn`.

## Acceptance criteria

- [ ] `MonoMidiIn` gains a fixed poly input wired to `GLOBAL_MIDI`
- [ ] Each tick, reads event count and events from the poly input
      using `MidiFrame` accessors
- [ ] Feeds decoded events into existing note-stack logic
- [ ] `ReceivesMidi` implementation removed from `MonoMidiIn`
- [ ] Existing tests updated and passing
- [ ] `cargo test` and `cargo clippy` pass
