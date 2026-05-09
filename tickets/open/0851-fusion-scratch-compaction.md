---
id: "0851"
title: Fusion phase 4 — scratch-region compaction, cache alignment, affinity-friendly layout
priority: low
created: 2026-05-09
epic: E141
adr: 0072
depends-on: "0850"
---

## Summary

Compact the scratch region of the cable pool so that scratch
indices are dense, ordered to match `active_indices`
(producer-before-consumer), and aligned to cache-line boundaries
where it pays off. Cycle-pair indices occupy reserved upper slots,
optionally grouped by SCC for future affinity-partitioning.

This is incremental optimisation on top of phase 3. Cancellable
independently if benchmarks do not motivate it.

## Acceptance criteria

- [ ] Scratch indices assigned in topo order: cable produced by
      `active_indices[i]` lands in the scratch region before any
      cable produced by `active_indices[j]` for `i < j`. Forward
      DAG traversal touches scratch slots in increasing index
      order, hitting each cache line once.
- [ ] Padding inserted at SCC boundaries (or at heuristic
      cluster boundaries) so that no cache line spans modules from
      two clusters that might in future be assigned to different
      threads. False sharing pre-empted.
- [ ] Cycle-pair indices clustered by SCC: pairs belonging to the
      same SCC's back edges sit contiguously in the cycle region.
      Within an SCC the order is arbitrary; across SCCs the order
      matches the condensation topo.
- [ ] Allocator simplification: scratch region is a single forward
      sweep over `active_indices`; cycle region is a separate
      forward sweep over the back-edge set. No free list, no
      fragmentation, no reuse.
- [ ] Benchmarks rerun against phase-3 baseline. Expected wins on
      cache-bound workloads (mod matrices, mixer trees, dense
      fan-out). Wins on cycle-light workloads should be small but
      positive; wins on cycle-heavy workloads may be zero
      (cycle-pair access is not the bottleneck there).
- [ ] All audio-integrity and feedback-regression tests still
      pass with bit-identical output.
- [ ] `just commit` passes; `just push` passes.

## Notes

Cache-line alignment matters most when a single tick of the
forward DAG touches enough scratch slots to span multiple lines.
For small patches the gain is invisible. For large patches
(hundreds of cables) it should be measurable.

The affinity-partitioning padding is forward-looking. Today the
engine is single-threaded, so SCC clusters do not need physical
isolation. Pre-empting false sharing now is cheap (a few padding
slots per SCC boundary) and means a future parallel scheduler can
assign clusters to threads without reshaping the pool.

If the SCC count is large enough that padding overhead dominates,
fall back to coarser cluster boundaries — the planner heuristic
can group small SCCs together up to a threshold.

This ticket may close as "not motivated" if phase-3 benchmarks
already show flat cache behaviour. ADR 0072's value proposition
is delivered by phases 1–2; phases 3–4 are bonus optimisation
work to be justified by measurement.

If parallelism becomes a real target later, this layout is the
foundation: SCC-clustered, cache-aligned, no false sharing
between potential thread partitions. Add the scheduler on top
without reshaping the pool.
