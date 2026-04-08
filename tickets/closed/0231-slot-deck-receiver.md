---
id: "0231"
title: SlotDeckReceiver — processing-thread handle
priority: high
created: 2026-03-31
---

## Summary

Implement `SlotDeckReceiver`: the processing-thread-side half of one `SlotDeck`
direction. It provides a simple pop/return API that the processing thread uses to
receive filled windows and return recycled buffers. It has no internal state
machine — all sequencing is the sender's concern.

## Acceptance criteria

- [ ] `SlotDeckReceiver` holds:
      - `filled_rx: rtrb::Consumer<FilledSlot>`
      - `recycled_tx: rtrb::Producer<Box<[f32]>>`
- [ ] `SlotDeckReceiver::pop(&mut self) -> Option<FilledSlot>` — non-blocking pop
      from `filled_rx`.
- [ ] `SlotDeckReceiver::recycle(&mut self, data: Box<[f32]>)` — push `data` to
      `recycled_tx`; on failure (channel full) drop `data` (the slot is lost from
      the pool; sender will degrade gracefully).
- [ ] A constructor function (e.g. `slot_deck(config, input_bufs, output_bufs)`)
      that wires up a matched `(SlotDeckSender, SlotDeckReceiver)` pair from
      pre-allocated buffers and returns both halves. `rtrb::RingBuffer` capacity
      is `config.pool_size()`.
- [ ] `SlotDeckReceiver` is `Send` (it crosses to the processing thread).
      `SlotDeckSender` is `!Send` (it stays on the audio thread — enforce via
      `PhantomData<*const ()>` or equivalent if needed).
- [ ] Compiles with `cargo clippy -- -D warnings`.

## Notes

Depends on T-0233 (stubs and ignored tests). No tests are exclusively
owned by this ticket — `round_trip_identity_inline` and
`round_trip_identity_threaded` are un-ignored in T-0232 once the full
pipeline is in place.

ADR 0023 §Processing-thread logic, §Channels.

The constructor is the natural place to ensure the two halves are created together
with matched channel ends and consistent pool size. Neither half is meaningful
without the other.

`recycle` dropping on full channel is intentional: the sender degrades gracefully
(opens fewer windows) rather than the processing thread blocking.
