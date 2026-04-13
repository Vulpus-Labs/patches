---
id: "0371"
title: MidiDrumset reads MIDI from backplane
priority: medium
created: 2026-04-13
---

## Summary

Refactor `MidiDrumset` to read MIDI events from the `GLOBAL_MIDI`
backplane slot (via `MidiFrame`) instead of implementing the
`ReceivesMidi` trait.

## Acceptance criteria

- [ ] `MidiDrumset` gains a fixed poly input wired to `GLOBAL_MIDI`
- [ ] Each tick, reads event count and events from the poly input
      using `MidiFrame` accessors
- [ ] Feeds decoded note events into existing drum trigger logic
- [ ] `ReceivesMidi` implementation removed from `MidiDrumset`
- [ ] Existing tests updated and passing
- [ ] `cargo test` and `cargo clippy` pass
