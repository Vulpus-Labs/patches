---
id: "0824"
title: Carry MIDI scratch overflow to next block
priority: medium
created: 2026-05-06
---

## Summary

`MidiScratch::write_midi_event` spills within-sample overflow forward
into later rows; if the spill target is past `block_size` (e.g. the
event arrived at the final sample with `MAX_EVENTS_PER_SAMPLE` already
filled, or earlier rows fully packed), the event is dropped and only
counted in `dropped`. This loses MIDI unpredictably under bursty input
(chord-stacked note-ons, dense CC sweeps, sysex-adjacent traffic).
Carry overflow into the next block instead so events are delivered
late by ≤1 block rather than silently lost.

## Acceptance criteria

- [ ] `MidiScratch` has a small pending queue for events that would
      otherwise overflow past `block_size`.
- [ ] `prepare_block` drains the queue into row 0 of the new block,
      applying the same spill-forward rule (queue→row 0→row 1…).
- [ ] If queue itself overflows (pathological saturation), bump
      `dropped` as today.
- [ ] Tests: tail-of-block overflow now reappears at sample 0 of the
      next block; multi-block saturation increments `dropped` only
      once the queue is full.
- [ ] No allocation on the audio thread; queue is preallocated.

## Notes

See [midi_scratch.rs:68-87](../../patches-engine/src/midi_scratch.rs#L68-L87)
for the current spill-then-drop logic. Queue capacity should be sized
to a plausible worst-case burst at one sample
(`MAX_EVENTS_PER_SAMPLE * k` for small `k`) — pick after measuring or
by analogy to host-control scratch sizing in 0816/0817.

Trade-off: carry-over delays events by up to one block (~ms at typical
buffer sizes). Acceptable vs. silent loss; document in the rustdoc.
