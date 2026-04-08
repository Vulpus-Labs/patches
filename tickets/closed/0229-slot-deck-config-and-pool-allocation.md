---
id: "0229"
title: FilledSlot, SlotDeckConfig, and buffer pool allocation
priority: high
created: 2026-03-31
---

## Summary

Define the foundational types for ADR 0023's `SlotDeck`: the `FilledSlot`
transfer type, the `SlotDeckConfig` parameter struct with validation, the pool
sizing formula, and the buffer allocation that happens once at construction time.
This ticket establishes the shared vocabulary for T-0230, T-0231, and T-0232.

## Acceptance criteria

- [ ] `SlotDeckConfig` struct with fields `window_size: usize`,
      `overlap_factor: usize`, `processing_budget: usize`; all must be powers of 2
      and `window_size >= overlap_factor`. Construction returns `Err` if any
      constraint is violated (no `unwrap`/`expect`).
- [ ] `SlotDeckConfig` exposes computed derived values:
      `hop_size()`, `total_latency()`, `pool_size()`.
      Formula: `hop = window_size / overlap_factor`,
      `pipeline_slots = processing_budget / hop` (minimum 1),
      `pool_size = next_power_of_two(2 * overlap_factor + pipeline_slots)`.
- [ ] `FilledSlot` struct: `start: u64`, `data: Box<[f32]>`.
- [ ] A `SlotPool::new(config: &SlotDeckConfig) -> (Vec<Box<[f32]>>, Vec<Box<[f32]>>)`
      (or equivalent) that pre-allocates `pool_size` input buffers and `pool_size`
      output buffers, each of length `window_size`, with all samples initialised
      to 0.0. No allocation occurs after this point.
- [ ] Lives in `patches-dsp`. No dependency on `patches-core`, `patches-modules`,
      or `rtrb` in this ticket (channels come in T-0230/T-0231).
- [ ] Unit tests: config validation rejects non-powers-of-two and invalid
      combinations; derived values are correct for the ADR example
      (`window_size=2048, overlap_factor=4, processing_budget=128` →
      `hop=512, total_latency=2176, pool_size=16`).

## Notes

Depends on T-0233 (stubs and ignored tests). Un-ignore on completion:
`config_rejects_non_power_of_two`, `config_rejects_zero_values`,
`config_rejects_window_smaller_than_overlap`,
`config_derived_values_correct`.

ADR 0023 §Configuration parameters, §Pool sizing.

`total_latency()` is `window_size + processing_budget` — two independently
meaningful quantities, not a single opaque value. Expose both components so
callers can report them separately.

`pipeline_slots` has a minimum of 1 to ensure at least one slot is available for
frames in flight even when `processing_budget < hop_size`.
