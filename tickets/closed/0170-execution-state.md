---
id: "0170"
title: Separate ExecutionState from ExecutionPlan
priority: medium
created: 2026-03-24
---

## Summary

`ExecutionPlan` currently conflates two concerns: it is both the planner→audio-thread
data-transfer object (slots, tombstones, new modules, parameter and port updates) and
the runtime driver of the audio tick (`sample_counter`, `tick()`). This ticket separates
them by introducing `ExecutionState`, which lives exclusively on the audio thread and
holds the sample counter and pre-allocated fixed-size arrays of raw module pointers.

On plan adoption the audio thread calls `ExecutionState::rebuild` to repopulate the
pointer arrays from the updated `ModulePool`. All subsequent ticks call
`ExecutionState::tick` directly — no pool index lookups, no bounds checks beyond
the pre-computed counts.

## Acceptance criteria

- [ ] `ExecutionState` in `patches-engine/src/execution_state.rs`:
  - `new(capacity: usize)` — pre-allocates both pointer arrays (no later allocation)
  - `rebuild(&mut self, plan: &ExecutionPlan, pool: &mut ModulePool)` — resets counts,
    writes raw pointers, resets `sample_counter = 0`; no allocation
  - `unsafe tick(&mut self, cable_pool: &mut CablePool<'_>)` — periodic dispatch +
    process loop; only iterates `[..count]` slots
- [ ] `ModulePool` gains `capacity()`, `as_ptr(idx)`, `as_periodic_ptr(idx)`
- [ ] `ExecutionPlan` no longer has `sample_counter` or `tick()`
- [ ] `AudioCallback` and `HeadlessEngine` use `ExecutionState::rebuild` +
  `ExecutionState::tick` instead of `ExecutionPlan::tick`
- [ ] `ExecutionState` exported from `patches-engine`
- [ ] `cargo test` passes, `cargo clippy` reports zero warnings

## Notes

Pointer arrays use `MaybeUninit<*mut dyn T>` so no placeholder fat pointer is needed.
Slots above the active high-watermark may hold stale/dangling pointers after module
tombstoning; they are never accessed because only indices `< count` are iterated.
`unsafe impl Send for ExecutionState` is required because raw pointers are not `Send`.
The safety invariant (rebuild always precedes tick; no tombstoning between the two)
is upheld by the plan-adoption sequences in `AudioCallback::receive_plan` and
`HeadlessEngine::adopt_plan`.
