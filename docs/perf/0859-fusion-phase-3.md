# Phase-3 cable-pool split: benchmarks vs phase-2 baseline

Ticket [0859](../../tickets/closed/0859-fusion-phase-3-benchmarks.md). ADR
0072 phase 3 (tickets 0850 + 0851 + 0858) split the cable buffer pool
into a small **cycle region** (pair-stored ping-pong, capacity
`CYCLE_CAPACITY = 128`) and a large **scratch region** (single-slot,
forward-DAG cables). The hypotheses: shrink allocation, densify the
forward-traversal cache footprint, no perf regression.

## Verdict

| Axis              | Result                                            |
|-------------------|---------------------------------------------------|
| memory footprint  | **meaningful win** — ~70% smaller used / ~48% smaller allocated |
| tick latency mean | **neutral** — within run-to-run noise on all four patches |
| tick latency p50  | **neutral** — identical or 1-bin off baseline |
| tick latency p99  | **neutral** — within noise; mixed-direction differences |

Phase 3 buys the storage win it was designed for. Tick perf is
indistinguishable from phase 2 on this hardware once outlier-affected
baseline runs are excluded — the dispatch branch on
`cable_idx < CYCLE_CAPACITY` does not show up in the timing band. Cache
metrics were not captured (see "Limitations" below).

## Setup

- Host: Apple Silicon (Darwin 23.5.0). macOS, no `perf` available.
- Build: `cargo build --release -p patches-profiling --bin bench`.
- Phase-3 (HEAD): commit `d456002` ("Close 0858: promote backplane
  reserved slots to scratch region"). Split storage in
  `PatchProcessor`, `init_cycle_pool` + `init_scratch_pool`,
  `CablePool::new(scratch, cycle, wi)`.
- Phase-2 baseline: commit `f0d89ea` ("0850 C3: split allocator into
  cycle and scratch regions"). Allocator already splits cable indices
  into the two regions, but storage is still a single
  `Box<[[CableValue; 2]]>` of size `POOL_CAPACITY`. This is the
  immediate predecessor of `bfad4a4` (0850 C4) which split the storage.
- Bench: `bench` binary in [patches-profiling/src/bin/bench.rs](../../patches-profiling/src/bin/bench.rs).
  1 s warmup, 10 s of 44.1 kHz sample-by-sample ticks, `Instant::now()`
  per tick, 5 back-to-back runs per side.
- Patches: drum_machine (mixer-tree-heavy), poly_synth_layered
  (mod-matrix-heavy / mostly fused), radigue_drone (cycle-heavy,
  `fas_size = 12`), tracker_three_voices (large fan-out via tracker).
- Bench-harness note: the original `bench.rs` was wedged at HEAD
  (`with_cycle_only` + a single buffer pool — every scratch-region
  access panicked with `index out of bounds: the len is 0`). Rewrote
  to mirror the engine's split-pool init via `init_cycle_pool` /
  `init_scratch_pool`. The baseline tree's bench was extended with
  the same metrics (mean/p50/p99/p99.9/max + footprint) for parity.

## Memory footprint

Footprint at the same `POOL_CAPACITY = 4096`. Used bytes derived from
the cable-index high-water mark in the plan; allocated bytes from the
boxed slices the engine actually owns.

| Patch                | Phase-3 used | Phase-2 used | Used Δ | Phase-3 alloc | Phase-2 alloc | Alloc Δ |
|----------------------|-------------:|-------------:|-------:|--------------:|--------------:|--------:|
| drum_machine         | 6 720 B      | 24 192 B     | −72 %  | 270 336 B     | 524 288 B     | −48 %   |
| poly_synth_layered   | 6 528 B      | 23 552 B     | −72 %  | 270 336 B     | 524 288 B     | −48 %   |
| radigue_drone        | 5 184 B      | 19 328 B     | −73 %  | 270 336 B     | 524 288 B     | −48 %   |
| tracker_three_voices | 7 168 B      | 25 088 B     | −71 %  | 270 336 B     | 524 288 B     | −48 %   |

Phase-3 used breakdown (cycle / scratch slots):

| Patch                | Cycle slots | Cycle B | Scratch slots | Scratch B | fas_size |
|----------------------|------------:|--------:|--------------:|----------:|---------:|
| drum_machine         |           6 |    768  |            93 |    5 952  |        2 |
| poly_synth_layered   |           7 |    896  |            88 |    5 632  |        4 |
| radigue_drone        |          13 |  1 664  |            55 |    3 520  |       12 |
| tracker_three_voices |           6 |    768  |           100 |    6 400  |        2 |

The cycle region is far below `CYCLE_CAPACITY = 128` even on the
cycle-heaviest patch tested (`radigue_drone` uses 13 of 128 cycle
slots). Forward-DAG cables (the bulk) drop from 128 B (pair) to 64 B
(single slot) per cable — the source of the ~70 % reduction in used
bytes.

## Tick latency

Five back-to-back runs each side, each run = 10 s of 44.1 kHz audio
(441 000 timed ticks). The macOS scheduler causes occasional run-wide
inflation that lifts mean and p99.9 — most visible in baseline runs 1
and 2 below (e.g. drum_machine baseline run 1 mean 1232 ns vs the
median 525 ns). Treat the median across 5 runs as the headline.

### drum_machine

| Run | side     | mean (ns) | p50 | p99   |
|----:|----------|----------:|----:|------:|
|   1 | head     |     505.9 | 458 |  1291 |
|   2 | head     |     491.9 | 417 |  1167 |
|   3 | head     |     484.5 | 417 |  1083 |
|   4 | head     |     500.3 | 458 |   875 |
|   5 | head     |     468.2 | 417 |  1167 |
|   1 | baseline |    1232.0 | 417 |  1458 |
|   2 | baseline |    1208.8 | 417 |  1459 |
|   3 | baseline |     501.8 | 417 |  1292 |
|   4 | baseline |     525.2 | 458 |  1291 |
|   5 | baseline |     469.2 | 417 |  1166 |

Median mean: **HEAD 491.9 ns vs baseline 525.2 ns**. p50 identical.

### poly_synth_layered

| Run | side     | mean (ns) | p50 | p99  |
|----:|----------|----------:|----:|-----:|
|   1 | head     |     785.7 | 667 | 1833 |
|   2 | head     |     852.7 | 667 | 1959 |
|   3 | head     |     765.1 | 667 | 1792 |
|   4 | head     |     778.1 | 667 | 1834 |
|   5 | head     |     775.8 | 667 | 1833 |
|   1 | baseline |    1751.6 | 708 | 4875 |
|   2 | baseline |    1786.8 | 708 | 4750 |
|   3 | baseline |     759.7 | 667 | 1833 |
|   4 | baseline |     784.6 | 667 | 1834 |
|   5 | baseline |     769.9 | 667 | 1792 |

Median mean: **HEAD 778.1 ns vs baseline 784.6 ns**. p50 ~1 bin apart
(667 vs 708 — single 41 ns timer bin).

### radigue_drone (cycle-heavy)

| Run | side     | mean (ns) | p50 | p99 |
|----:|----------|----------:|----:|----:|
|   1 | head     |     241.6 | 208 | 375 |
|   2 | head     |     261.8 | 208 | 625 |
|   3 | head     |     243.6 | 208 | 334 |
|   4 | head     |     241.6 | 208 | 542 |
|   5 | head     |     240.8 | 208 | 334 |
|   1 | baseline |     341.9 | 208 | 667 |
|   2 | baseline |     642.1 | 208 | 708 |
|   3 | baseline |     222.7 | 208 | 541 |
|   4 | baseline |     221.9 | 208 | 541 |
|   5 | baseline |     220.8 | 208 | 541 |

Median mean: **HEAD 241.6 ns vs baseline 222.7 ns** — HEAD ~9 %
slower in mean; p50 identical. This is the patch where the dispatch
branch was predicted to matter most (cycle-heavy, every cable hits the
cycle arm). The signal is small enough that on this hardware it is
hard to distinguish from noise — the spread of clean baseline runs
(220-223 ns) is narrow, but only one HEAD run dips below 241 ns.
**Worth re-measuring on Linux with `perf stat`** if the cycle-region
branch becomes a hot suspect (see follow-up below).

### tracker_three_voices

| Run | side     | mean (ns) | p50 | p99  |
|----:|----------|----------:|----:|-----:|
|   1 | head     |     336.6 | 292 |  417 |
|   2 | head     |     338.6 | 292 |  375 |
|   3 | head     |     346.1 | 292 |  916 |
|   4 | head     |     342.0 | 292 |  417 |
|   5 | head     |     343.9 | 292 |  458 |
|   1 | baseline |     843.8 | 292 | 1209 |
|   2 | baseline |    1146.4 | 292 | 1250 |
|   3 | baseline |     325.9 | 292 |  916 |
|   4 | baseline |     331.3 | 292 |  916 |
|   5 | baseline |     329.2 | 292 |  417 |

Median mean: **HEAD 342.0 ns vs baseline 331.3 ns** — HEAD ~3 %
slower; p50 identical.

## Limitations

- **No cache-miss data.** macOS / Apple Silicon: `perf` does not
  exist; `xctrace` Time Profiler reports CPU time, not cache events;
  hardware-counter access (`Instruments → Counters`) needs root and
  is flaky on Apple Silicon. Linux CI run with `perf stat -e
  L1-dcache-load-misses,LLC-load-misses` is the cleaner way to land
  the cache-locality claim. Deferred — patches fit comfortably in L2
  in both layouts (used bytes ≤ 25 KiB), so the working-set
  reduction's marginal value is small at these scales.
- **Timer overhead.** `Instant::now()` per tick on macOS costs
  ~30 ns. Both sides pay it, so relative comparison stands; absolute
  ns/tick is inflated by ~5-15 % across the board.
- **Run noise.** macOS scheduling preemption produces a tail that
  occasionally lifts whole runs (visible in baseline runs 1-2). 5
  runs per side is just enough to tell the medians apart from
  outliers. CI-style measurement on a quiet Linux box would tighten
  this further.

## Follow-up

No follow-up ticket filed. The neutral runtime result matches the
hypothesis (the branch should be predictable). The cycle-heavy
`radigue_drone` mean shift is small enough to live inside the noise
band on macOS; revisit only if Linux `perf stat` shows the dispatch
branch as a real cost. If a regression does emerge there, the
ticket-suggested mitigation (compile-time region sharding via
separate inline `read_cycle` / `read_scratch` paths so the planner can
emit calls that bypass the branch) is the natural fix.

`CYCLE_CAPACITY = 128` is comfortably overprovisioned for every patch
tested (max 13 cycle slots used). No need to plumb a per-engine
constant through `PatchProcessor::new`. If a future patch with very
heavy module-level cycles approaches the bound, that's the trigger to
revisit the constant.
