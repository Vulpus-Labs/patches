---
id: "0816"
title: Host-control scratch buffer + smoothing + per-sample memcpy
priority: high
created: 2026-05-04
epic: E136
depends_on: "0815"
superseded_by: "0817"
---

> **Superseded by ticket 0817 (2026-05-05).** This ticket placed the
> SoA scratch, AoS frame, smoothing pipeline, and per-sample memcpy on
> the `HostControl` *module*. Working on ticket 0811 (CLAP integration)
> exposed the shape mismatch: `prepare_block` is driven by host events
> arriving once per audio buffer, not by the cable graph, and putting
> it on a singleton module forced `module_pool` lookups with no name
> index. ADR 0068 §2 amendment 2026-05-05 relocates the pipeline to
> `PatchProcessor`; ticket 0817 carries out the move and reduces the
> module to "read backplane lane, write output port".

## Summary

Implement the per-block host-control scratch pipeline from ADR 0068
§2: SoA fill of CLAP automation events, in-place one-pole smoothing
of knob / slider rows, transpose to an AoS frame, and per-sample
`memcpy` into the four contiguous control-region cable slots.

Replaces the placeholder backplane-cell writes from ticket 0809.
Lifts ADR 0057 §4 ("sub-block automation parked") — sample-accurate
automation falls out of this design.

This ticket does **not** ingest CLAP events. The audio thread reads
from a synthetic pre-baked event list for testing; ticket 0811 wires
the real CLAP `process_event` path into the same scratch fill.

## Acceptance criteria

- [ ] Per-engine-instance scratch + frame buffers, allocated once at
      activation: SoA `[64][MAX_BLOCK]` and AoS `[MAX_BLOCK][64]` of
      `f32`. Per-channel tail state `[f32; 64]`.
- [ ] Step-fill pass: takes `&[(channel: u8, sample_offset: u16,
      value: f32)]` (sorted), produces SoA scratch with carry-forward
      semantics. Unaffected channels carry the previous-block tail.
- [ ] Smoothing pass: per-row one-pole with α from sample rate × ~5 ms
      time constant, applied in-place over rows where
      `kind.smoothed()`. Toggle / trigger rows skip.
- [ ] Trigger handling: rows are zero-filled with `1.0` at each event
      sample; downstream `Trigger`-shaped reads see exactly the
      ADR 0057 trigger semantics.
- [ ] Transpose pass: SoA `[64][N]` → AoS `[N][64]`. Single pass.
- [ ] Per-sample `HostControl::tick()` does
      `pool[hc_base..hc_base+4][wi].copy_from_slice(...)` against the
      AoS frame row. One memcpy; no per-channel loop in the inner
      tick.
- [ ] Tail state updated at end of `process()` from frame's last row.
- [ ] Tests:
      - empty event list → all rows carry-forward, frame contains
        only previous tail repeated;
      - single event mid-block → step until offset, value after
        offset (or smoothed ramp for knob / slider);
      - smoothing converges to target within expected time at the
        configured time constant;
      - trigger event produces exactly one nonzero sample per
        event in the row;
      - kind dispatch verified by inspecting the row contents for
        each kind on the same event stream;
      - end-to-end: scratch → cable pool → downstream module reads
        match the AoS frame for every sample of the block.
- [ ] ADR 0057 §4 amended: sample-accurate automation is implemented
      via this scratch design; remove the parked-status note.
- [ ] `just inner -p patches-engine -p patches-modules` passes.
- [ ] `just push` passes, including the determinism / hash-stability
      suite (smoothing must be deterministic).

## Notes

- Smoothing α: `α = 1 - exp(-1 / (sample_rate * tau))` with
  `tau ≈ 5 ms`. Compute once per (sample-rate × tau) change; ship in
  the plan-adoption ring alongside the `id_to_channel` table when
  ticket 0811 lands.
- Per-channel kind mask (`smoothed: bool` per channel) also ships in
  the plan; until 0811, build it from a static descriptor used by
  tests.
- AoS frame layout: `[N][64]` of `f32`. Use `bytemuck::cast_slice` (or
  an equivalent zero-cost reinterpret) to view a `&[f32; 64]` row as
  `&[[f32; 16]; 4]` for the per-sample `copy_from_slice` into the
  four cable slots. Depends on ticket 0815 having dropped the
  `CableValue` enum.
- No allocation on the audio thread. All buffers preallocated at
  activation; `MAX_BLOCK` taken from the engine environment.
