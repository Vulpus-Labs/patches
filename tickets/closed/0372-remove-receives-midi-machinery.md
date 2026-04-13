---
id: "0372"
title: Remove ReceivesMidi trait and dispatch machinery
priority: medium
created: 2026-04-13
---

## Summary

With all MIDI modules now reading from the `GLOBAL_MIDI` backplane,
the `ReceivesMidi` trait, `as_midi_receiver()` on `Module`, and the
engine's MIDI dispatch infrastructure are dead code. Remove them.

## Acceptance criteria

- [ ] `ReceivesMidi` trait removed from `patches-core/src/midi.rs`
- [ ] `as_midi_receiver()` removed from the `Module` trait
- [ ] `midi_modules: PtrArray<dyn ReceivesMidi>` removed from
      `ReadyState` / `StaleState`
- [ ] `deliver_midi()` and `dispatch_midi_events()` removed from
      `ReadyState`
- [ ] `deliver_midi()` removed from `PatchProcessor`
- [ ] `midi_receiver_indices` removed from `ExecutionPlan` and
      planner
- [ ] `as_midi_receiver_ptr()` removed from `ModulePool`
- [ ] `as_midi_receiver()` removed from `ModuleHarness`
- [ ] CLAP plugin no longer calls `deliver_midi()`
- [ ] `dispatch_midi()` on `PatchProcessor` simplified (no longer
      calls `deliver_midi` per event)
- [ ] All planner tests referencing `midi_receiver_indices` removed
      or updated
- [ ] `cargo test` and `cargo clippy` pass
