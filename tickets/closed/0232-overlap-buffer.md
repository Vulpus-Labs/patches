---
id: "0232"
title: OverlapBuffer — audio-thread composite with overlap-add
priority: high
created: 2026-03-31
---

## Summary

Implement `OverlapBuffer`: the top-level audio-thread struct that composes two
`SlotDeck` instances (one in each direction) and presents a simple per-sample
`write` / `read` interface with overlap-add reconstruction. This is the type
that a module author constructs and calls on the audio thread.

## Acceptance criteria

- [ ] `OverlapBuffer` holds:
      - A `SlotDeckSender` for the write direction (audio → processor).
      - A `SlotDeckSender` for the read direction (processor → audio), renamed
        internally as the *result sender* — or equivalently, the receiver side of
        the read-direction deck that the audio thread owns.

  More concretely: two pairs are constructed at init time:
      - Write deck: `(write_sender: SlotDeckSender, write_receiver: SlotDeckReceiver)`.
        Audio thread keeps `write_sender`; `write_receiver` is given to the processor.
      - Read deck: `(read_sender: SlotDeckSender, read_receiver: SlotDeckReceiver)`.
        Processor thread keeps `read_sender`; audio thread keeps a
        `ResultConsumer` (the `filled_rx` / `recycled_tx` ends of the read deck)
        for draining completed result windows.

- [ ] `OverlapBuffer::new(config: SlotDeckConfig) -> (OverlapBuffer, ProcessorHandle)`
      — constructs both decks, splits off the two processing-thread halves
      into a
      `ProcessorHandle { write_rx: SlotDeckReceiver, read_tx: SlotDeckSender }`,
      and returns both. All allocation happens here.

- [ ] `OverlapBuffer::write(&mut self, sample: f32)` — delegates to the write-deck
      sender. Advances `write_head` (already tracked inside `SlotDeckSender`).

- [ ] `OverlapBuffer::read(&mut self) -> f32` — implements ADR 0023 §Read-side
      logic:
      1. Drain the result channel: pop newly-completed output windows and append
         to `active_outputs: [Option<(u64, Box<[f32]>)>; MAX_OVERLAP]`. If
         `active_outputs` is full, push the displaced (or incoming) slot to the
         recycle channel.
      2. Sum contributions: for each active output window whose range contains
         `read_head`, accumulate `slot.data[read_head - slot.start]`.
      3. Expire windows: for each active output window where
         `read_head >= slot.start + window_size - 1`, push its buffer to the
         recycle channel and remove it from `active_outputs`.
      4. Increment `read_head` and return the sum.

  `read_head` starts at `write_head - total_latency` (i.e. begins negative
  relative to zero, clamped — or equivalently, the first `total_latency` calls
  to `read` return 0.0 while the pipeline fills).

- [ ] `ProcessorHandle` is `Send`. `OverlapBuffer` is `!Send`.
- [ ] No allocation in `write` or `read`.
- [ ] Compiles with `cargo clippy -- -D warnings`.

## Notes

Depends on T-0233 (stubs and ignored tests). Un-ignore on completion:
`startup_silence`, `round_trip_identity_inline`,
`round_trip_identity_threaded`, `late_frame_discarded`.

ADR 0023 §Read-side logic, §Latency.

The `read_head` initialisation is subtle. The simplest correct approach: initialise
`read_head = 0` and `write_head = total_latency`, so the write head is pre-advanced
by the full latency at construction. This means the first `total_latency` writes
happen before any result windows can cover `read_head = 0`, producing natural
startup silence without special-casing negative indices.

`MAX_OVERLAP` for the `active_outputs` array: the read deck's sender is the
processor, so at most `overlap_factor` result windows are dispatched per
`window_size` samples. The same bound applies.
