---
id: "0230"
title: SlotDeckSender — write-side state machine
priority: high
created: 2026-03-31
---

## Summary

Implement `SlotDeckSender`: the audio-thread-side half of one `SlotDeck`
direction. It owns the free slot pool, maintains the set of currently-open
windows, and drives the per-sample write path — opening new windows at each
hop boundary, writing samples, dispatching completed windows, and draining the
recycle channel.

## Acceptance criteria

- [ ] `SlotDeckSender` holds:
      - `free_slots: Vec<Box<[f32]>>` (pre-populated at construction, never
        reallocated)
      - `active: [(u64, Box<[f32]>); MAX_OVERLAP]` fixed-capacity active windows
        (or equivalent stack-allocated structure bounded by `overlap_factor`)
      - `write_head: u64`
      - `filled_tx: rtrb::Producer<FilledSlot>`
      - `recycled_rx: rtrb::Consumer<Box<[f32]>>`
- [ ] `SlotDeckSender::write(&mut self, sample: f32)` implements the per-sample
      logic from ADR 0023 §Write-side logic:
      1. At each hop boundary (`write_head % hop_size == 0`): drain `recycled_rx`
         into `free_slots`; if a free slot is available, claim it and open a new
         active window with `start = write_head`.
      2. For each active window whose range contains `write_head`: write `sample`
         into `slot.data[write_head - slot.start]`.
      3. For each active window where `write_head == slot.start + window_size - 1`:
         attempt `filled_tx.push(FilledSlot { start, data })`; on failure (channel
         full) return `data` to `free_slots`. Remove from `active`.
      4. Increment `write_head`.
- [ ] All failure paths are silent degradation — no `unwrap`, no blocking.
- [ ] `SlotDeckSender::new` takes the pre-allocated buffers (from T-0229) and the
      `rtrb` producer/consumer ends. Does not allocate.
- [ ] `rtrb` added to `patches-dsp/Cargo.toml` (ask if not already present).
- [ ] Compiles with `cargo clippy -- -D warnings`.

## Notes

Depends on T-0233 (stubs and ignored tests). Un-ignore on completion:
`write_overload_full_channel`,
`pool_starvation_degrades_gracefully`.

ADR 0023 §Write-side logic.

The `active` array is bounded by `overlap_factor`. At any moment at most
`overlap_factor` windows are open simultaneously. A fixed `[Option<...>;
MAX_OVERLAP]` array (const generic or a runtime-checked small vec) avoids
allocation. If using const generics, `MAX_OVERLAP` is threaded through from
`SlotDeckConfig::overlap_factor` at construction — either as a const parameter or
by asserting at runtime that `active.len() >= overlap_factor`.

Step 1 only runs at hop boundaries to amortise the channel drain cost. This is
O(overlap_factor) work every `hop_size` samples, negligible on the audio thread.
