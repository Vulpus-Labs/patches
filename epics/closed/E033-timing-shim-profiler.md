---
id: "E033"
title: Timing-shim profiler
status: closed
priority: medium
created: 2026-03-24
tickets:
  - "0182"
  - "0183"
  - "0184"
---

## Summary

Replace the current `profile` binary's per-slot isolation approach with a
`TimingShim` that wraps every module at construction time and records wall-clock
time for each `process()` and `periodic_update()` call into a shared
`TimingCollector`. The patch then runs through the normal headless-engine tick
loop — full signal chain, correct upstream inputs, periodic updates fired on the
real schedule — and timing is gathered passively inside the shims.

This fixes two problems with the current profiler:
- Modules were run in isolation so inputs went stale after the first tick.
- `periodic_update` was never called, so filter coefficient-recompute cost was
  invisible.

All new code lives in `patches-profiling`; no changes to `patches-core` or
`patches-engine` are required.

## Tickets

- [T-0182](../tickets/open/0182-timing-collector.md) — `[lib]` target + `TimingCollector`
- [T-0183](../tickets/open/0183-timing-shim.md) — `TimingShim: Module + PeriodicUpdate`
- [T-0184](../tickets/open/0184-profile-plain-tick-loop.md) — Wire shims into `profile.rs`; plain tick loop; collector report
