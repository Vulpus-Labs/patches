---
id: "0740"
title: Planner — detect structural-param edits and trigger instance rebuild
priority: medium
created: 2026-04-28
epic: "E126"
adrs: ["0060", "0044"]
depends_on: ["0734", "0737"]
---

## Summary

Wire the planner to detect structural-param diffs across plan rebuilds
and route them through the existing instance-swap path
(ADR 0044). When a structural value changes for an existing module,
the planner constructs a fresh `Box<dyn Module>` via
`Module::prepare(...)`, swaps it into the slot, and retires the old
instance. The descriptor is unchanged, so no graph rewire, no cable
reallocation, no port re-binding.

## Acceptance criteria

- [ ] Planner diff captures structural-param changes per instance,
      separately from realtime-param changes (which continue down the
      `ParamFrame` swap path) and from descriptor-changes (which
      remain a graph rewire).
- [ ] On structural-edit, planner calls
      `Module::prepare(env, descriptor, instance_id, &new_structural)`
      to mint a new instance, then enqueues the swap on the existing
      adoption path.
- [ ] Failure path: if `prepare` returns `Err`, the plan rebuild is
      rejected with the underlying `BuildError`; the running engine
      keeps the old instance and surfaces the error to the host.
- [ ] Unit test: build a patch with a `convolution_reverb`, swap the
      `ir_path` to a different file, confirm a new instance is
      constructed and the convolver state reflects the new IR.
- [ ] Unit test: structural-edit failure (e.g. invalid `ir_path`)
      leaves the running engine intact and surfaces `BuildError`.
- [ ] `cargo test -p patches-planner -p patches-engine` passes.
- [ ] No new audio-thread allocations introduced (the `prepare` call
      runs on the control thread; only the arc swap touches the
      audio thread, exactly as for graph-edit hot-reload today).

## Notes

Decision: structural rebuilds mint a **fresh `InstanceId`**, identical
to descriptor-changing rebuilds (e.g. shape/channels change). Observers
keyed on instance id will see the discontinuity, which matches the
existing semantics for any module-identity change. No advantage was
found to preserving the id, and avoiding a same-slot tombstone-then-
install path keeps the adoption order untouched. ADR-0060 follow-up
note updated accordingly.

## Status

Implemented (2026-04-29):

- `Module::build` / `Registry::create` / `ModuleBuilder::build` /
  `DylibModuleBuilder::build` take `&StructuralParams` and forward it
  to `Module::prepare`; the transitional empty-carrier branch in
  `patches-core/src/modules/module.rs` is removed.
- `NodeState` carries the structural snapshot; `classify_nodes` /
  `make_decisions` accept a `structural_by_node` map and reclassify
  surviving nodes whose structural blob differs as `Install`.
- Planner exposes `Planner::build_with_structural` and
  `PatchBuilder::build_patch_with_structural`; existing entry points
  remain (default to an empty structural map per node).
- Unit tests in `patches-planner/src/builder/tests/structural.rs`
  cover the rebuild path and the prepare-failure path.

Graph→interpreter→planner threading of the structural map is split
out to ticket 0746 (depends on this ticket).
