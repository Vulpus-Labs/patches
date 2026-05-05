# ADR 0069 — Block-rate MIDI scratch buffer

**Date:** 2026-05-05
**Status:** Proposed
**Related:**
[ADR 0048 — MIDI source and routing modules](0048-midi-source-and-routing-modules.md),
[ADR 0057 — Host control as boundary-crossing cables](0057-host-control-cables.md),
[ADR 0068 — Untagged cable pool and host-control scratch buffer](0068-untagged-cable-pool-and-host-control-scratch-buffer.md)

## Context

`PatchProcessor::write_midi` packs events into the **current** sample's
`GLOBAL_MIDI` poly slot during the per-sample tick loop. Overflow
beyond `MidiFrame::MAX_EVENTS` per sample bleeds into the next sample's
frame via an internal stash. Audio callers walk their event source
inside the per-sample loop and call `write_midi` at the right offset.

Two shapes feed events:

- **Player** — an external MIDI input thread pushes events into an
  SPSC ring; the audio callback drains a per-sample window. Events
  arrive continuously and asynchronously; the per-sample drain shape
  was the natural fit when this was written.
- **CLAP** — the host hands the plugin the entire `clap_input_events`
  list at the top of `process()`. There is no streaming uncertainty;
  every event for the buffer is known before tick begins.

Per-sample event ingest has costs:

- Per-tick branch on event-queue state, even for buffers with zero
  events.
- Conflates DSP and event ingest in the audio callback's hot loop;
  the loop reads cleaner when events are pre-staged.
- Forces overflow-spill bookkeeping (`midi_overflow`, drain-and-shift)
  to live alongside the per-sample tick rather than at the natural
  block boundary.

ADR 0068 §2 (host control) already established a block-rate scratch
pattern: events feed an AoS frame at the top of the buffer, the
per-sample tick memcpys a row into the backplane region. That pattern
generalises cleanly to MIDI.

## Decision

### 1. MIDI events stage into a block-rate AoS frame on the processor

`PatchProcessor` owns `MidiScratch`:

```rust
struct MidiScratch {
    /// AoS frame indexed `t * MAX_EVENTS_PER_SAMPLE + i`.
    /// Pre-allocated to MAX_BLOCK × MAX_EVENTS_PER_SAMPLE entries.
    frame: Box<[MidiEvent]>,
    /// Per-sample event count, indexed by `t`.
    counts: Box<[u8]>,
    /// Block size of the most recent prepare; per-tick cursor.
    block_size: usize,
    sample_idx: usize,
}
```

Approximate cost: `2048 × 16 × 3 bytes ≈ 96 KB` for the events plus
`2048 bytes` for the counts. Single-digit cache lines per row;
trivial for the audio thread.

### 2. Two-phase ingest, one-shot per-tick flush

- `processor.write_midi_event(sample_offset, event)` stamps one event
  into the row at `sample_offset`. Overflow within a single sample
  spills to `sample_offset + 1`, recursing as needed (audio thread
  is the sole writer; spilling is a tight loop, not a re-entrant
  data structure).
- `processor.prepare_midi_block(frames)` resets the per-tick cursor
  and applies a final overflow sweep. Idempotent on an empty frame.
- In `tick()` before module dispatch, the processor packs `counts[t]`
  events from row `t` into `midi_poly` and flushes to `GLOBAL_MIDI`.
  Single sequential write; no overflow drain interleaved with module
  execution.

### 3. Player and CLAP both go through the scratch

- **Player.** The audio callback drains the SPSC for the buffer's
  time window, calling `write_midi_event(offset, event)` once per
  drained event. Late arrivals (event timestamp earlier than
  `block_start` because the source thread was slow) clamp to
  `offset = 0` rather than being dropped. `prepare_midi_block(frames)`
  fires once before the per-sample loop.
- **CLAP.** The plugin walks `clap_input_events` once, calls
  `write_midi_event(header.time, event)` for each MIDI message, and
  fires `prepare_midi_block(frames)`. The per-sample tick loop holds
  no event-iteration state.

### 4. Existing `write_midi` is retired

The per-sample-write API and `midi_overflow` stash are removed. The
overflow-into-next-sample semantics survive as the natural
spill-into-next-row behaviour of the AoS frame.

## Consequences

**Positive**

- Per-sample tick loop holds no event-queue state. Pure DSP from the
  module dispatch downward.
- Symmetric with host control (ADR 0068 §2 amended). One pattern, one
  pair of `write_*_event` / `prepare_*_block` calls per boundary input.
- CLAP integration becomes a single sweep over `clap_input_events`
  before the tick loop, with no per-sample event lookups.
- Player retains correct semantics; the SPSC drains at block boundary
  rather than per-sample window. Late events still get sample-accurate
  placement within the next block.

**Negative**

- 96 KB / instance for the AoS frame. Negligible in absolute terms
  but worth flagging.
- Existing `write_midi` callers in tests and integration harnesses
  must move to the new API. Touch surface is small (the function is
  thin).

**Neutral**

- Determinism preserved: the AoS frame is a deterministic projection
  of the input event list; per-sample dispatch sees the same events
  in the same order.

## Order of execution

1. Implement `MidiScratch` + new processor surface alongside the
   existing `write_midi` (do not retire it yet).
2. Migrate the player's audio callback to drain the SPSC at block
   boundary into `write_midi_event`.
3. Migrate the CLAP plugin's `process()` to the new path (incidental
   to ticket 0817 follow-up).
4. Migrate integration tests and harnesses.
5. Remove `write_midi`, `midi_overflow`, and the per-sample drain in
   `tick()`.
