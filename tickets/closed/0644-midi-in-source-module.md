---
id: "0644"
title: New MidiIn source module
priority: medium
created: 2026-04-23
---

## Summary

Add `MidiIn`: pure MIDI source. Reads from a backplane slot and writes
events to a single `midi` output port. No voice state, no CV.

## Acceptance criteria

- [ ] New module `MidiIn` in `patches-modules/src/midi_in.rs`
      (the file currently holding `MonoMidiIn` will move per 0643)
- [ ] One output port `midi` of `CableKind::Midi`
- [ ] `slot` parameter (int, default `GLOBAL_MIDI`) selecting backplane source
- [ ] Module-level test: events written to backplane appear on the port
- [ ] Module doc-comment in standard form (CLAUDE.md convention)
- [ ] Registered in `patches-modules/src/lib.rs`

## Notes

ADR 0048. Depends on 0641 (`CableKind::Midi`) and 0643 (frees the name).
