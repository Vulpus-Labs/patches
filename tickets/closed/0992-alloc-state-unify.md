---
id: "0992"
title: Unify allocation transformation/state types (buffer + module)
priority: low
created: 2026-05-29
---

## Summary

The allocation layer carries two sibling type pairs that differ by one or
two fields:

| Transformation output | Carried state | Delta |
|-----------------------|---------------|-------|
| `BufferAllocation { output_buf, to_zero, cycle_freelist, cycle_hwm, scratch_hwm }` | `BufferAllocState { output_buf, cycle_freelist, cycle_hwm, scratch_hwm }` | `to_zero` is per-build, not carried |
| `ModuleAllocDiff { slot_map, freelist, next_hwm, tombstoned }` | `ModuleAllocState { pool_map, freelist, next_hwm }` | `tombstoned` is per-build; `slot_map` ↔ `pool_map` rename |

`PatchBuilder::build_draft` ends with explicit field-by-field re-packing
of both pairs (lines ~768-779 today): each output field is read out of the
transformation type and written into the state type. This is the
re-bundling ADR 0081's "no churn" rule prohibits.

Three workable shapes:

1. **`From` impls.** Add `impl From<BufferAllocation> for BufferAllocState`
   and `impl From<ModuleAllocDiff> for ModuleAllocState`. Re-pack collapses
   to `state.buffer_alloc = buf_alloc.into()`.
2. **Composed shape.** `BufferAllocation { state: BufferAllocState, to_zero:
   Vec<usize> }`. State extraction is `buf_alloc.state`; the per-build delta
   stays beside it. Same for `ModuleAllocDiff { state: ModuleAllocState,
   tombstoned: Vec<usize> }`.
3. **Drop the second type.** `BufferAllocation` carries the per-build
   `to_zero` only; `output_buf` / freelist / hwm live directly on
   `PlannerState` and are mutated in place. Lower-churn but breaks the
   immutable-state-thread invariant the planner currently maintains.

(2) is closest to the "compose, don't re-map" rule. (1) is the smallest
diff. Choose at implementation time; preserve the immutability of carried
state either way.

## Acceptance criteria

- [ ] End-of-`build_draft` no longer constructs `BufferAllocState` or
      `ModuleAllocState` field-by-field. The carried state flows out via
      either `.into()`, field access on a composed shape, or equivalent —
      not literal field-by-field copy.
- [ ] `BufferAllocState::scratch_hwm` — currently documented as
      diagnostic-only and "not consulted by the next allocation pass" —
      either flows naturally from the new composed shape or is removed if
      no test/diagnostic still reads it. (See 0993 for the dead-field audit
      criterion.)
- [ ] `slot_map` ↔ `pool_map` naming is consistent — pick one
      (`pool_map` is the more semantic name; `slot_map` is the older
      transformation-output name) and use it across both types.
- [ ] No clones of the surviving-state fields are introduced at the boundary;
      ownership flows transformation → state by move.
- [ ] Audio goldens bit-identical; `just push` green.

## Notes

Part of epic **E162**. Independent of 0987 / 0988 / 0989 / 0991 — runs in
parallel. Low priority: the current re-pack is correct, just visibly the
re-mapping ADR 0081 named as a smell. Trivial to land; trivial to leave.
