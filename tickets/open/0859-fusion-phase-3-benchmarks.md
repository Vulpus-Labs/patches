---
id: "0859"
title: Benchmark phase 3 cable pool split vs phase 2 baseline
priority: low
created: 2026-05-10
epic: E141
adr: 0072
depends-on: "0850"
---

## Summary

Ticket 0850 split the cable pool into a cycle region (pair-stored,
preserving feedback delay) and a scratch region (single-slot,
forward-DAG cables). The expected wins are reduced footprint and
denser cache lines along the common-case forward traversal. Confirm
with measurements on representative patches.

## Acceptance criteria

- [ ] Capture phase-2 baseline (commit immediately preceding 0850 C4)
      across the four representative patches:
      - drum kit (mixer-tree-heavy)
      - mod-matrix-heavy patch (mostly fused, tiny FAS)
      - resonant feedback patch (cycle-heavy)
      - large fan-out (single producer to many consumers)
- [ ] Capture phase-3 measurements at HEAD on the same patches.
- [ ] Metrics per patch:
      - average tick duration (μs)
      - p99 tick duration (μs)
      - L1 / L2 cache miss rate via `perf stat`
      - cable-pool memory footprint (bytes)
- [ ] Document results in `docs/perf/0859-fusion-phase-3.md` with a
      verdict: meaningful win, neutral, or regression.
- [ ] If regression: file a follow-up ticket characterising the cause
      (likely the scratch dispatch branch in `read_*`/`write_*` if
      the predictor cannot fold it). Consider compile-time region
      sharding (separate inline functions per region) if so.

## Notes

`CYCLE_CAPACITY = 128` is hardcoded as of 0850. If benchmarks suggest
different sizes for production vs CI, plumb a per-engine constant
through `PatchProcessor::new` (constructor arg).

The drum kit and mod-matrix patches stress the scratch path; the
feedback patch stresses the cycle path; the fan-out patch stresses
both via shared producer ports.

Reuse the profiling harness in `patches-profiling/src/bin/{bench,profile}.rs`
to drive measurements. Capture `perf stat -e L1-dcache-load-misses,LLC-load-misses`
on Linux; on macOS use `xctrace record --template "Time Profiler"`.
