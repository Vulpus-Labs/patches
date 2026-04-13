---
id: "0384"
title: Fix dispatch_midi batch size constant
priority: low
created: 2026-04-13
---

## Summary

In `patches-engine/src/processor.rs` line 303, `dispatch_midi` uses a hardcoded
batch size of 16, while `MidiFrame::MAX_EVENTS` is 5. Events beyond
`MAX_EVENTS` are silently dropped by `write_midi`. The batch array is
over-allocated and the silent truncation is not obvious.

## Acceptance criteria

- [ ] Batch size references `MidiFrame::MAX_EVENTS` (or a derived constant) instead of a magic 16
- [ ] Add a comment explaining the relationship between batch size and frame capacity
- [ ] No behaviour change (overflow already handled by `write_midi`), but the intent is clearer

## Notes

The `write_midi` method already handles overflow into the next sample's frame,
so a batch larger than `MAX_EVENTS` isn't harmful — but using 16 when the
constant is 5 is misleading. Using `MidiFrame::MAX_EVENTS * 2` (to fill
current frame + overflow) or just `MidiFrame::MAX_EVENTS` with a note would
be clearer.
