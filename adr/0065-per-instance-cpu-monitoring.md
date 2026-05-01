# ADR 0065 — Per-instance CPU monitoring

**Date:** 2026-05-01
**Status:** Proposed
**Related:**
[ADR 0043 — Cable tap observation](0043-cable-tap-observation.md),
[ADR 0053 — Observation three-thread split](0053-observation-three-thread-split.md),
[ADR 0055 — Observation bringup via ratatui patches-player](0055-observation-bringup-via-ratatui-player.md)

## Context

`patches-player`'s ratatui UI currently has no module-level diagnostic
view. A static list of loaded modules adds no signal; what would
matter is per-instance CPU cost, so a user can see which modules
dominate the budget and where headroom sits.

The audio engine dispatches every module on every sample (1-sample
cable delay; per-sample round-robin through the active set). Naive
per-call `Instant::now` brackets — ~25ns/read on macOS — at 48 kHz
across N modules cost too much and, more importantly, perturb the
measurement: typical cheap modules tick in 30–150 ns, so a 50 ns
bracket pair adds 30–150 % bias.

This ADR records the design that came out of working through those
constraints: cheap, low-bias, observer-driven monitoring that
collapses to the existing hot path when disabled.

## Decision

### Sampling shape

Per audio block, when monitoring is enabled:

1. Take timestamp at start of block.
2. Pick one **selected module slot** for the block (round-robin over
   the active set across blocks).
3. Take timestamp pair around the **periodic phase**.
4. In the per-sample dispatch, time the selected module's `tick`.
   Within a block, time only every Kth sample (decimation,
   K = 16 by default). Accumulate into a per-block `Duration`.
5. Take timestamp at end of block.
6. Push one record onto an SPSC channel to the observer.

The dispatch loop is split to avoid a per-call branch inside the hot
loop:

```text
tick_range(0..sel)        // untimed, byte-identical to current loop
tick_one_timed(sel)       // timestamp pair every Kth sample
tick_range(sel+1..n)      // untimed
```

The same split is used for the periodic phase (one timed module out
of `periodic_indices`).

When monitoring is disabled (engine config `monitor: None` at engine
construction), the existing single-loop dispatch path is used
verbatim — no atomic, no branch per sample, no allocation.

### Decimation arithmetic

Observer multiplies, never divides on the audio thread, to preserve
precision at small sample counts:

```text
estimated_module_cost_per_block
    = module_accum * block_samples / module_samples_timed
```

The audio thread sends raw `module_accum`, `module_samples_timed`,
and `block_samples`; the observer scales. Decimation rate (K) lives
in audio config and is invisible to the observer.

### Timestamp source

`std::time::Instant` for v1. Cost: ~5 timestamps for block / periodic
bounds plus 2 × (128/16) = 16 per block for the selected module ≈
~24 reads/block, ~9 k reads/sec at 48 kHz / 128-sample blocks.
At 25 ns/read this is ~0.025 % CPU. Bias on timed samples ~25 %; over
many blocks under uniform module RR, per-module estimate variance is
dominated by sample count, not bias.

A future cargo feature may swap to `rdtsc`/`rdtscp` (~8 ns) on
x86_64 with calibrated TSC frequency. Apple Silicon's `cntvct_el0`
runs at 24 MHz (≈42 ns resolution) and offers no improvement over
`mach_absolute_time`; on macOS, `Instant` stays. The observer-facing
type remains `Duration` regardless of source.

### SPSC payload

```text
MonitorBlock {
    block_duration:        Duration,
    periodic_duration:     Duration,
    module_slot:            usize,
    module_accum:          Duration,   // sum of timed samples
    module_samples_timed:  u32,
    block_samples:         u32,
}
```

### Slot → instance name mapping

Slot indices are assigned by the **control thread** in
`ModuleAllocState::diff` (`patches-planner/src/state/alloc.rs`),
not the audio thread. The audio thread is a consumer of the index
allocation, not a producer.

Instance names (`QName` from the DSL) live in the planner's
ephemeral `nodes` map during build but are not currently persisted
into `ExecutionPlan` or `ModulePool`. The builder will be extended
to construct, alongside the plan, a `Vec<QName>` keyed by slot index
(and the parallel periodic-subset names).

This vector is **not** stored inside `ExecutionPlan`. It is passed
as a separate argument to `PatchProcessor::adopt_plan(plan, names)`
so its lifetime is orthogonal to the plan's. On adopt, the audio
thread moves it through a drop ladder:

1. Push to monitor SPSC. Observer drops on its thread.
2. SPSC full / no observer subscribed → push to existing plan
   cleanup channel. GC thread drops.
3. Cleanup channel full (sized for plan churn — should not happen)
   → drop in audio thread as last resort.

When monitoring is disabled, the builder passes `None` and no
allocation, no traversal, no drop work happens.

### Observer responsibilities

- Receive `PlanMeta { names, types }` on plan swap; rebuild
  slot-indexed name and type tables.
- Aggregate per-block `MonitorBlock` records into per-instance
  rolling estimates (% of block budget, mean over window).
- Aggregate by module *type* by summing per-instance estimates
  grouped by type name. The UI wants both views: per-instance to
  find the specific hot module, per-type to spot a class of modules
  (e.g. all filters) dominating the budget.

The slot-indexed metadata sent on plan swap therefore carries both
names and types:

```text
PlanMeta {
    names: Vec<QName>,        // instance names, slot-indexed
    types: Vec<&'static str>, // module type names, slot-indexed
}
```

Type names are already `&'static str` on `NodeState::module_name`
in the planner; collecting them alongside `QName` is free. Routing
follows the same drop ladder as names (passed as a separate arg to
`adopt_plan`, not stored on `ExecutionPlan`).

### UI

`patches-player`'s ratatui adds a CPU-monitor tab when monitoring is
configured at startup. No tab when not configured — zero UI cost in
the default path. Tab shows per-instance %, sorted descending.

## Consequences

### Positive

- Off path is byte-identical to today: no atomic, no branch in the
  per-sample dispatch loop, no allocation.
- Per-instance granularity from day one. Type-level rollups are
  observer-side, free.
- Decimation gives constant overhead independent of N modules
  (only the selected one is timed).
- Slot index assignment was already control-thread; no audio-thread
  bookkeeping needed for naming.

### Negative

- Round-robin selection means each module is sampled 1/N of the
  time. Convergence to a stable estimate scales with N. For large
  patches (>50 modules) the UI may need a longer window to settle.
- Bias on timed samples (~25 % with `Instant`, ~10 % with `rdtsc`)
  systematically over-reports. Acceptable as a comparative measure;
  not a precision benchmarking tool.
- `Vec<QName>` drop in the cleanup channel adds reload-time work,
  not steady-state work. Reload is already non-RT-critical.

### Out of scope

- Per-cable / per-port timing.
- Sub-block resolution timing (would change the dispatch shape).
- Counting allocations or other non-time costs.
- A protocol for the observer to *change* monitoring config
  (decimation rate, selection strategy) at runtime. v1 fixes both
  at engine construction.
