---
id: "0862"
title: Drop `cycle_slot_start`; surface `cycle_hwm`/`scratch_hwm` instead
priority: low
created: 2026-05-10
epic: E143
depends-on: "0860"
---

## Summary

`cycle_slot_start` is carried as a `pub` field on both `PlanDecisions`
([patches-planner/src/state/mod.rs:185](patches-planner/src/state/mod.rs#L185))
and `ExecutionPlan`
([patches-planner/src/builder/mod.rs:268](patches-planner/src/builder/mod.rs#L268))
purely for tests and diagnostics. Both docstrings explicitly note the
engine does not consume it. Its formula (`SINK_SLOTS +
dyn_cycle_count`) is identical to the `cycle_hwm` already tracked in
`BufferAllocState`. After 0860 the SINK_SLOTS framing also goes away
(sinks move to scratch); the field becomes meaningless even as
diagnostic.

Delete the field from both structs. If tests need a stable
diagnostic on plan shape, expose `cycle_hwm` and `scratch_hwm` as
public read-only accessors on `ExecutionPlan` and `BufferAllocation`.

## Acceptance criteria

- [x] `cycle_slot_start` removed from `ExecutionPlan` and
      `PlanDecisions`. All ETL and field-init sites updated.
- [x] If tests asserted against `cycle_slot_start`, port them to the
      new accessor (or to `BufferAllocState::cycle_hwm` directly).
- [x] `ExecutionPlan::cycle_hwm() -> usize` and
      `ExecutionPlan::scratch_hwm() -> usize` accessors added,
      reading from the stored `BufferAllocation` (or stored on the
      plan if the allocation isn't retained).
- [x] `just push` clean.

## Notes

Coordinates with 0863 (bench.rs HWM re-derive) — the accessors added
here are exactly what 0863 needs. Land 0862 first, then 0863 consumes
them.
