---
id: "0863"
title: Replace bench.rs cable-watermark re-derive with plan accessors
priority: low
created: 2026-05-10
epic: E143
depends-on: "0862"
---

## Summary

`patches-profiling/src/bin/bench.rs::watermarks` (added in 0859)
walks every slot's `unscaled_inputs`, `scaled_inputs`, and
`output_buffers` to recompute the per-region max `cable_idx`. The
plan already knows these — `BufferAllocState` tracks `cycle_hwm` and
`scratch_hwm`. The re-derive will silently drift if cable_idx
semantics shift again (and 0860 will shift them).

Replace `watermarks` with calls to the accessors added in 0862.

## Acceptance criteria

- [x] `bench.rs::watermarks` removed.
- [x] Footprint columns in the bench output read from
      `plan.cycle_hwm()` / `plan.scratch_hwm()` (or equivalent).
- [x] The pre-0850 commit at which footprint baselines were captured
      ([docs/perf/0859-fusion-phase-3.md](docs/perf/0859-fusion-phase-3.md))
      is noted to use the older derivation — no re-baseline needed if
      the values agree.
- [x] `just push` clean.

## Notes

Trivial after 0862 lands.
