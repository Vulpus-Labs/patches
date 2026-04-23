---
id: "0643"
title: Rename MidiIn/PolyMidiIn to MidiToCv/PolyMidiToCv
priority: medium
created: 2026-04-23
---

## Summary

Rename the voice-tracker modules to reflect that they are MIDI→CV
converters, freeing the `MidiIn` name for the new pure source module.

| Old | New |
| ---------- | ------------ |
| `MidiIn` | `MidiToCv` |
| `PolyMidiIn` | `PolyMidiToCv` |

Keep registry aliases for the old names for one release so user patches
keep parsing.

## Acceptance criteria

- [ ] `MonoMidiIn` struct and file renamed → `MidiToCv` /
      `patches-modules/src/midi_to_cv.rs`
- [ ] `PolyMidiIn` struct and file renamed → `PolyMidiToCv` /
      `patches-modules/src/poly_midi_to_cv.rs`
- [ ] Module names registered as `"MidiToCv"` and `"PolyMidiToCv"`
- [ ] Aliases `"MidiIn"` and `"PolyMidiIn"` registered (with deprecation
      note in code comment)
- [ ] In-tree `.patches` files (vintage, fixtures, examples) updated to
      new names
- [ ] Module reference docs (`docs/src/modules/`) regenerated/updated
- [ ] `cargo clippy` and `cargo test` clean

## Notes

ADR 0048. Coordinate with 0644 which takes over the `MidiIn` name.
