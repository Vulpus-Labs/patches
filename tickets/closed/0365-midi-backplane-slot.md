---
id: "0365"
title: MIDI backplane slot and engine integration
priority: medium
created: 2026-04-12
---

## Summary

Reserve a backplane poly slot for MIDI events and wire the engine
to populate it from incoming MIDI. The CLAP plugin and standalone
player write MIDI events into the slot using `MidiFrame` each tick.
This replaces the engine's direct `ReceivesMidi` dispatch as the
primary path for MIDI into the module graph.

## Acceptance criteria

- [ ] New `GLOBAL_MIDI` constant in `patches-core/src/cables.rs`
      (slot 10, first available reserved slot)
- [ ] Slot initialised as `CableValue::Poly([0.0; 16])` in
      buffer pool setup
- [ ] `PatchProcessor` gains `write_midi(&mut self, events: &[MidiEvent])`
      that packs up to 5 events per sample into `GLOBAL_MIDI`
      using `MidiFrame`
- [ ] Events beyond 5 per sample are deferred to the next sample
      (FIFO overflow buffer, pre-allocated)
- [ ] CLAP plugin calls `write_midi()` with sample-accurate MIDI
      events before each tick
- [ ] Standalone `patch_player` calls `write_midi()` from its
      existing MIDI input path
- [ ] `GLOBAL_MIDI` is cleared (count = 0) at the start of each
      tick before writing
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- See ADR 0033 for the MIDI-over-poly design.
- The overflow buffer handles the rare case of >5 simultaneous
  events by spilling to the next sample. At 48 kHz this adds
  ~21 μs of latency — inaudible.
- The `ReceivesMidi` trait is not removed in this ticket; both
  paths coexist during migration.
