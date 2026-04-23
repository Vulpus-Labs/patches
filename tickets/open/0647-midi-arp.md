---
id: "0647"
title: MidiArp arpeggiator module
priority: medium
created: 2026-04-23
---

## Summary

Add `MidiArp`: arpeggiator over the set of currently-held notes,
clocked by an external trigger input. On each clock pulse, emits a
note-off for the previous step and a note-on for the next, walking the
selected pattern. Empty hold set emits nothing. Non-note events pass
through.

## Acceptance criteria

- [ ] New module `MidiArp` in `patches-modules/src/midi_arp.rs`
- [ ] Ports: `midi` in (backplane fallback), `clock` trigger in,
      `midi` out
- [ ] Parameters:
    - `pattern` (enum: `up`, `down`, `up_down`, `random`, `as_played`)
    - `octaves` (int, 1..=4, default 1)
    - `gate_length` (float, 0.0..=1.0, default 0.5; fraction of
      observed clock period — track interval between pulses)
- [ ] Note-off for current step is emitted when `gate_length` of the
      period elapses, or when the next pulse arrives, whichever first
- [ ] Adding/removing held notes mid-pattern updates the working set
      cleanly; releasing all notes silences the next step
- [ ] Tests: each pattern walks correctly; octave expansion works;
      release-all stops output without stuck notes; clock period
      tracking adapts to tempo changes
- [ ] Module doc-comment in standard form
- [ ] Registered

## Notes

ADR 0048. Depends on 0641, 0642. Clock input uses `CableKind::Trigger`
(ADR 0030 / 0047 — sample-boundary fine for arp timing).
