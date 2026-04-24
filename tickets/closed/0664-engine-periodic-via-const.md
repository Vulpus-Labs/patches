---
id: "0664"
title: Engine collects periodic slots via WANTS_PERIODIC; drop as_periodic_ptr
priority: high
created: 2026-04-24
epic: E114
adr: 0052
depends_on: ["0663", "0665"]
---

## Summary

Switch `ExecutionPlan::periodic_indices` population to use
`Module::WANTS_PERIODIC` at the construction site where concrete module
types are known, and dispatch `periodic_update` via `&mut dyn Module`
at tick time. Delete `ModulePool::as_periodic_ptr` and the
`PtrArray<dyn PeriodicUpdate>` stored in `ReadyState`.

## Acceptance criteria

- [ ] Plan builder records slot `idx` in `periodic_indices` iff the
      concrete module's `WANTS_PERIODIC` is true. Evaluated at
      construction, not via a trait-object method.
- [ ] `ReadyState::periodic_modules: PtrArray<dyn PeriodicUpdate>`
      removed; `periodic_slots: Vec<usize>` is the only periodic state.
- [ ] Periodic tick dispatch goes through `ModulePool` to call
      `Module::periodic_update` on the boxed trait object — no raw
      pointer, no `transmute`, no lifetime laundering.
- [ ] `ModulePool::as_periodic_ptr` deleted.
- [ ] `cargo test -p patches-core -p patches-modules -p patches-dsp
      -p patches-engine` green.
- [ ] Integration test covering a periodic module (e.g. an SVF in a
      patch) still runs correctly and produces unchanged output.

## Notes

This ticket depends on 0665 so module impls already expose
`WANTS_PERIODIC`. If we need to land engine-side first, gate on
`as_periodic().is_some()` transiently, but prefer sequencing
0663 → 0665 → 0664 to avoid a middle state.

The hot path stays the same: pre-filtered `periodic_slots` vec walked
once per periodic interval. Only difference is the per-slot indirection
goes via the boxed `Module` (already cached) instead of a separate
cached pointer.
