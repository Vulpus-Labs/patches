---
id: "0648"
title: MidiDelay pure MIDI delay module
priority: medium
created: 2026-04-23
---

## Summary

Add `MidiDelay`: buffers MIDI events and re-emits them after a fixed
sample delay. Note and non-note events delayed identically. Buffer is
bounded; on overflow the oldest event is dropped, with a matching
note-off suppressed/synthesised as needed to avoid stuck notes.

## Acceptance criteria

- [ ] New module `MidiDelay` in `patches-modules/src/midi_delay.rs`
- [ ] Ports: `midi` in (backplane fallback), `midi` out
- [ ] Parameter: `delay_samples` (int, 0..=`MAX_DELAY`, default e.g.
      4800 ≈ 100ms at 48k). Pre-allocated ring buffer sized for
      `MAX_DELAY` — no allocation on parameter change
- [ ] Bounded event buffer (e.g. 256 events); overflow drops oldest
- [ ] If a dropped event is a note-on, suppress its later note-off; if
      a note-on was already emitted but its note-off is dropped on
      overflow, synthesise a note-off at output to avoid stuck notes
- [ ] Tests: events emerge at correct delay; overflow behaviour; no
      stuck notes after overflow stress; param change mid-stream
      doesn't lose buffered events that have already been scheduled
- [ ] Module doc-comment in standard form
- [ ] Registered

## Notes

ADR 0048. Depends on 0641, 0642. Real-time-safe: no allocation in
`process()`.
