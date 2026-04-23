---
id: "0641"
title: Add CableKind::Midi core plumbing
priority: medium
created: 2026-04-23
---

## Summary

Introduce `CableKind::Midi` as a typed view over the existing
`[f32; 16]` poly buffer used by `MidiFrame`. Same buffer layout, same
debouncing logic in `MidiInput` / `MidiOutput`; distinct kind tag so
graph validation rejects accidental wiring of audio-poly outputs into
MIDI inputs.

## Acceptance criteria

- [ ] `CableKind::Midi` variant added in `patches-core/src/cables/mod.rs`
- [ ] `port_kind_tag` in `param_layout` extended (new tag value)
- [ ] Graph compatibility match in `graphs/graph` accepts `(Midi, Midi)`
- [ ] `MidiInput` / `MidiOutput` constructors take a `Midi` cable slot
      (poly buffer underneath, unchanged)
- [ ] Tests: graph rejects Poly→Midi and Midi→Poly connections
- [ ] `cargo clippy` and `cargo test` clean

## Notes

ADR 0048. Buffer pool, frame size, and packed encoding all unchanged —
this is purely a type discipline change.
