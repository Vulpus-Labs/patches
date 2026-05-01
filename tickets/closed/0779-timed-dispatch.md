---
id: "0779"
title: Optional timed dispatch in engine, raw block records on SPSC
priority: medium
created: 2026-05-01
---

## Summary

Add per-block CPU monitoring to the engine: round-robin module
selection, decimated per-sample timestamping of the selected module,
periodic-phase timing, and SPSC dispatch of raw `MonitorBlock`
records to an observer. Off path is byte-identical to today's
dispatch loop.

See [ADR 0065](../../adr/0065-per-instance-cpu-monitoring.md) for
the full design and `MonitorBlock` payload.

## Acceptance criteria

- [ ] Engine config gains `monitor: Option<MonitorConfig>` set at
  construction. `MonitorConfig` carries decimation rate K (default
  16) and the SPSC producer endpoint.
- [ ] When `monitor` is `None`, `ExecutionPlan::tick` runs the
  current single-loop dispatch unchanged. Verified by inspection
  and a microbenchmark or codegen check.
- [ ] When `monitor` is `Some`, dispatch splits into
  `tick_range(0..sel)` / `tick_one_timed(sel)` /
  `tick_range(sel+1..n)`. Same shape for the periodic phase.
- [ ] Selected slot rotates round-robin per block over the active
  set; resets on plan adopt.
- [ ] Per block, push one `MonitorBlock` to the SPSC. Channel full
  → drop record silently (audio thread must not block).
- [ ] Decimation: only every Kth sample of the selected module is
  bracketed; accumulator + count + block-sample-count sent raw to
  the observer (no division on audio thread).
- [ ] `Instant::now` used for v1; behind a feature flag, swap to
  `rdtsc` on x86_64 with calibrated TSC frequency. Apple Silicon
  stays on `Instant`.
- [ ] No allocations on the audio thread in the timed path.
- [ ] Test: with a contrived plan of N modules of known relative
  cost, observer-side estimates converge to expected ratios within
  X blocks (tolerance documented in test).

## Notes

- `tick_one_timed` does the rdtsc/Instant pair around `module.tick`
  every Kth sample within the block; other samples within the block
  it dispatches without timing.
- Round-robin selection state is one `usize` on `PatchProcessor`,
  modulo active count, advanced once per block.
- Plan adoption (ticket 0778) is the synchronization point for the
  name table; selection idx must be validated against the new
  active count after adopt.
