---
id: "0706"
title: tap frame format — lane-major block frames + sample_time
priority: high
created: 2026-04-26
---

## Summary

Two retrofits to the tap frame format shipped in 0700, both required
by 0701 (observer crate) and the broader observation pipeline:

1. **Sample-major block layout.** Current frame = `[f32; MAX_TAPS]`
   (one sample × N lanes, one push per tick). Refactor to
   `[[f32; MAX_TAPS]; BLOCK]` — i.e. accumulate `BLOCK` of the
   existing per-sample frames into one block frame, push once per
   block. Producer-side write per sample stays a contiguous 128 B
   memcpy (cache-friendly: 2 lines/sample). Observer transposes
   into lane-major work buffers on receipt, off the audio thread,
   so processors (peak, RMS, FFT, scope) see contiguous
   `&[f32; BLOCK]` per lane for SIMD.
2. **Sample timestamp.** Add `sample_time: u64` (monotonic sample
   count from engine init, pointing to the first sample in the
   block). Required for UI timing sync (scope sweep alignment,
   meter timestamps, multi-lane sync). Resets on engine rebuild
   (which is the only time SR can change).

## Rationale

- Per-sample pushes cost ~64× the ring overhead of block pushes at
  BLOCK=64. Block pushes also align with the audio thread's natural
  loop iteration.
- Sample-major within a frame forces the observer to transpose into
  lane buffers before any per-lane reduction. Lane-major eliminates
  the transpose; observer slices `frame.lanes[i]` directly.
- Frame size grows from 128 B (32×4) to 8 KiB (32×64×4). Ring of
  4–8 frames = 32–64 KiB. Acceptable; bounded.

## Acceptance criteria

- [ ] Keep existing `TapFrame = [f32; MAX_TAPS]` as the live
  per-sample backplane type (modules continue to write into it
  every tick, unchanged). Add a new `TapBlockFrame` struct for the
  ring transport:
  - `samples: [[f32; MAX_TAPS]; TAP_BLOCK]` (sample-major within
    block; row i = full backplane snapshot for sample i of this
    block)
  - `sample_time: u64` (monotonic sample index of `samples[0]`)
  - `TAP_BLOCK` = const in `patches-core`. Initial value: 64.
    Independent of host audio block size (host blocks are arbitrary).
- [ ] `PatchProcessor` accumulates `TAP_BLOCK` per-tick backplane
  snapshots into a `tap_block: TapBlockFrame` field, indexed by
  `tap_block_idx: usize`. Each tick: `tap_block.samples[idx] =
  tap_backplane` (single 128 B memcpy, contiguous). On
  `idx == TAP_BLOCK`, push the block frame and reset. Unused lanes
  are zeroed in the live backplane by the Tap module — they fall
  through to the block frame as zeros for free.
- [ ] `sample_time` captured at `idx == 0` (block start), from a
  fresh monotonic `tap_sample_counter: u64` field on
  `PatchProcessor` (separate from `sample_count` which wraps at
  2^16 for transport). Counter increments once per tick, resets on
  engine rebuild. Frame's `sample_time` = the index of
  `samples[0]`.
- [ ] No allocation, no extra atomics on the audio path. `try_push_
  frame` stays `#[inline]`, lock-free, fire-and-forget.
- [ ] Drop counter semantics unchanged: full ring → bump every
  per-slot counter, drop the frame.
- [ ] Tests:
  - Existing 0700 ring tests adapted to `TapBlockFrame`.
  - `sample_time` strictly monotonic across consecutive frames at
    `TAP_BLOCK` stride.
  - Sample-major fill: write distinct value per (sample, lane),
    drain, confirm `frame.samples[i][j]` carries expected value.
  - Block boundary: push not emitted until `TAP_BLOCK` ticks have
    accumulated; partial block on shutdown is fine to drop.
- [ ] Doc comment on `TapBlockFrame` covers: sample-major
  rationale (producer cache-friendliness — 128 B memcpy/sample, 2
  cache lines), observer transposes to lane-major for SIMD,
  unused-lane zeroing as Tap module's responsibility,
  `sample_time` resets on engine rebuild, u64 wraparound
  non-issue.

## Notes

Oversampling consideration from 0700's original notes: pushing one
frame per `tick()` made the tap rate follow oversampling for free.
With block frames, the same property holds — the block is whatever
the oversampled engine's block is. No change needed.

Consumer side (`patches-observation`, ticket 0701) consumes this
format directly; 0701 should land after this retrofit.

## Cross-references

- 0700 — original frame ring ticket (closed; this retrofits its
  output).
- 0701 — observer crate (consumer of the new format).
- ADR 0053 §5 — frame ring.
- ADR 0054 §6 — manifest.
- ADR 0056 — observer pipeline and frame layout (defines this format).
