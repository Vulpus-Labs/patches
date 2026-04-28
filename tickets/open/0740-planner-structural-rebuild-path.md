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
and route them through the existing arc-table instance-swap path
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

The instance-id stays stable across structural rebuilds — observers
keyed on instance id (taps, meters) should not see a discontinuity
beyond what the swap itself implies. Document this explicitly in the
ADR-0060 follow-up notes if behaviour differs from
descriptor-changing rebuilds.
