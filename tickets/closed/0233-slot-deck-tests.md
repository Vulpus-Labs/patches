---
id: "0233"
title: SlotDeck stubs and ignored test suite
priority: high
created: 2026-03-31
---

## Summary

Establish the skeleton that TDD for the rest of E044 will build against. Write
minimal empty stubs for every public type and function (enough to compile), then
write the full test suite with every test marked `#[ignore]`. Subsequent tickets
(T-0229 through T-0232) un-ignore tests as they implement each piece.

No real logic is implemented in this ticket — stubs may `todo!()` or return
`unimplemented!()`. The goal is a compiling crate with all tests visible and
ignored.

## Acceptance criteria

- [ ] The following stub types exist in `patches-dsp::slot_deck` (or a submodule)
      and compile:
      - `SlotDeckConfig` — fields `window_size`, `overlap_factor`,
        `processing_budget`; a `new(...)` returning `Result`; methods
        `hop_size()`, `total_latency()`, `pool_size()`.
      - `FilledSlot` — fields `start: u64`, `data: Box<[f32]>`.
      - `SlotDeckSender` — `write(&mut self, f32)`.
      - `SlotDeckReceiver` — `pop(&mut self) -> Option<FilledSlot>`,
        `recycle(&mut self, Box<[f32]>)`.
      - `slot_deck(config, ...) -> (SlotDeckSender, SlotDeckReceiver)` constructor.
      - `OverlapBuffer` — `new(SlotDeckConfig) -> (OverlapBuffer, ProcessorHandle)`,
        `write(&mut self, f32)`, `read(&mut self) -> f32`.
      - `ProcessorHandle` — `pop(&mut self) -> Option<FilledSlot>`,
        `recycle(&mut self, Box<[f32]>)`, `push_result(&mut self, FilledSlot)`.
      - `ProcessorHandle: Send`, `OverlapBuffer: !Send`.

- [ ] The following tests exist in `patches-dsp/tests/slot_deck.rs` (or
      `patches-dsp/src/slot_deck/tests.rs`), all marked `#[ignore]`:
      - `config_rejects_non_power_of_two`
      - `config_rejects_zero_values`
      - `config_rejects_window_smaller_than_overlap`
      - `config_derived_values_correct` (ADR example: 2048/4/128 → hop=512,
        latency=2176, pool_size=16)
      - `startup_silence`
      - `round_trip_identity_inline` (processor simulated synchronously)
      - `round_trip_identity_threaded` (real spawned thread)
      - `late_frame_discarded`
      - `write_overload_full_channel`
      - `pool_starvation_degrades_gracefully`

- [ ] `cargo test -p patches-dsp` passes (all real tests pass; ignored tests are
      skipped).
- [ ] `cargo clippy -- -D warnings` clean.
- [ ] `patches-dsp/src/lib.rs` (or `mod.rs`) exports the new module.

## Notes

Stub bodies should use `todo!()` rather than `unimplemented!()` so that if a test
accidentally runs against an incomplete stub it fails with a clear message rather
than an abort.

Each test should carry a comment identifying which ticket implements it:

```rust
#[test]
#[ignore] // implemented in T-0229
fn config_rejects_non_power_of_two() { ... }
```

This makes it easy to know which tests to un-ignore when picking up each ticket.
