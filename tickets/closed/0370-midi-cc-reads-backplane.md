---
id: "0370"
title: MidiCc reads MIDI from backplane
priority: medium
created: 2026-04-13
---

## Summary

Refactor `MidiCc` to read MIDI events from the `GLOBAL_MIDI`
backplane slot (via `MidiFrame`) instead of implementing the
`ReceivesMidi` trait.

## Acceptance criteria

- [ ] `MidiCc` gains a fixed poly input wired to `GLOBAL_MIDI`
- [ ] Each tick, reads event count and events from the poly input
      using `MidiFrame` accessors
- [ ] Feeds decoded CC events into existing value tracking
- [ ] `ReceivesMidi` implementation removed from `MidiCc`
- [ ] Existing tests updated and passing
- [ ] `cargo test` and `cargo clippy` pass
