---
id: "0254"
title: SlotDeck pool exhaustion recovery and edge cases
priority: medium
created: 2026-04-02
---

## Summary

The SlotDeck system (OverlapBuffer, ProcessorHandle, SlotVec) is well-tested at
the integration level via `tests/slot_deck.rs`, but several internal edge cases
lack unit-level coverage. The existing `pool_starvation_degrades_gracefully` and
`write_overload_full_channel` tests confirm no panics, but do not verify that the
system *recovers* after starvation or that output quality degrades predictably.

## Acceptance criteria

- [ ] Test that after pool starvation (all slots consumed, none returned), the
      system resumes correct output once slots are returned — i.e. recovery is
      clean with no stuck state.
- [ ] Test that a slow processor thread (delayed `push_result`) causes graceful
      degradation (silence or repeated frames) rather than corruption.
- [ ] Test OverlapBuffer behaviour when the result channel is full — verify that
      excess results are handled without panic or memory growth.
- [ ] Test SlotVec recycling: after a burst of allocations and recycles, the pool
      returns to its original size and all recycled buffers are reusable.
- [ ] All new tests are in `patches-dsp/tests/slot_deck.rs` or as unit tests in
      `slot_deck.rs`.

## Notes

The existing `pool_starvation_degrades_gracefully` test only checks that the
system doesn't panic during starvation. The new tests should verify what
*happens* to the output stream and that recovery is seamless.
