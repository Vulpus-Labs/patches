---
id: "0818"
title: MidiScratch on processor — block-rate AoS frame + write_midi_event
priority: high
created: 2026-05-05
epic: E137
depends_on: "0817"
---

## Summary

Add `MidiScratch` to `PatchProcessor` per ADR 0069. New API:

- `processor.write_midi_event(sample_offset: u16, event: MidiEvent)` —
  stamps one event into the AoS frame at the given sample row. Within
  a sample, overflow beyond `MidiFrame::MAX_EVENTS` spills into the
  next row.
- `processor.prepare_midi_block(frames: usize)` — resets the per-tick
  cursor; runs a final overflow sweep so a packed last sample doesn't
  silently drop tail events.

`tick()` per-sample, before module dispatch, packs the row's events
into `midi_poly` and flushes to `GLOBAL_MIDI`. Replaces the existing
per-sample `write_midi` + `midi_overflow` drain.

The existing `write_midi(events: &[MidiEvent])` API stays alongside
the new one for the duration of this ticket so callers can migrate
incrementally. Ticket 0819 retires it.

## Acceptance criteria

- [ ] `MidiScratch` struct in `patches-engine`:
      - AoS frame `Box<[MidiEvent]>` sized
        `MAX_HOST_CONTROL_BLOCK × MAX_EVENTS_PER_SAMPLE`,
      - per-sample `counts: Box<[u8]>`,
      - block_size + sample_idx cursors.
      Allocated once at processor construction.
- [ ] `MAX_EVENTS_PER_SAMPLE` constant in `patches-core` (set to
      `MidiFrame::MAX_EVENTS` to match the per-sample backplane
      capacity; the spill-into-next-row policy means deeper bursts
      land on subsequent samples, not get dropped).
- [ ] `processor.write_midi_event(offset, event)`:
      - Out-of-range offsets clamp to `block_size - 1` (late-arrival
        semantics from the player path);
      - Within-sample overflow spills into the next row, recursing
        until a row with capacity is found or the block end is hit;
      - Events that don't fit anywhere in the block are dropped to a
        debug counter (`cleanup_overflow_count` or sibling).
- [ ] `processor.prepare_midi_block(frames)` resets the cursor and
      clamps `frames` to `MAX_HOST_CONTROL_BLOCK`.
- [ ] `tick()` host-control flush gains a sibling MIDI flush:
      - Read `counts[sample_idx]` events from the AoS row,
      - Pack into `midi_poly` via the existing `MidiFrame::write_event`,
      - Stamp the count, write to `GLOBAL_MIDI[wi]`.
- [ ] Existing `write_midi(events)` keeps working: bridge it onto
      `write_midi_event(0, ev)` (or onto `prepare_midi_block` inputs
      via the row at index 0). Callers that depend on per-sample
      semantics continue to work; behaviour is observably identical
      because the AoS frame still gets packed into the same backplane
      slot per sample.
- [ ] `tick()` no longer drains `midi_overflow`. The struct field
      stays for one ticket but is unused; 0819 removes it.
- [ ] Tests:
      - `write_midi_event` at offset N stamps the row at N;
      - within-sample overflow lands in row N+1 in arrival order;
      - `prepare_midi_block(frames)` followed by `frames` ticks
        flushes each row in order;
      - empty block: per-sample MIDI frames carry zero events;
      - parity: `write_midi(&[ev])` followed by tick produces the
        same backplane state as `write_midi_event(0, ev)` +
        `prepare_midi_block(1)` + tick.
- [ ] `just inner -p patches-core -p patches-modules -p patches-dsp -p patches-engine`
      passes.
- [ ] Determinism / hash-stability suite passes (no observable change
      at the patch level).

## Notes

- Spill recursion is bounded by `MAX_HOST_CONTROL_BLOCK`; in practice
  buffers don't approach the per-sample MIDI cap, so the inner loop
  short-circuits on the first row with capacity.
- `MidiScratch::new` rustdoc flags the ~96 KB allocation and links to
  ADR 0069.
- Keep `MidiScratch` and `HostControlScratch` shaped alike (same
  `prepare_block` / `next_row` cadence, same cursor semantics) so
  later refactors can fold them onto a shared trait if useful.
