---
id: "0976"
title: Lock-in tests for action-phase ExecutionPlan output and make_decisions orchestration
priority: high
created: 2026-05-29
---

## Summary

Two stages have ~no direct coverage and are about to be refactored under E160:

- **`make_decisions`** (patches-planner/src/state/mod.rs) — the decision-phase
  orchestrator. Tested today only indirectly (the 0974 regression + integration
  tests). No direct assertions on the bundled `PlanDecisions` it produces.
- **The action phase** (`PatchBuilder::build_patch_with_meta`,
  patches-planner/src/builder/mod.rs) — turns `PlanDecisions` into an
  `ExecutionPlan` + `PlannerState`. Only narrow slices are tested
  (`partition_inputs`, structural, ffi-offset); the core output (port objects,
  slots, param frames, tombstones, `to_zero`) has no direct assertions.

Add behaviour-lock-in tests now, before the refactor. The action phase needs a
registry, so introduce a **minimal test registry** with a couple of hand-built
modules (a multi-output module and a delay-like module, reusing the 0974 test
descriptors) rather than the full `default_registry()`.

This is a test-only ticket: no production logic changes.

## Acceptance criteria

- [ ] Direct `make_decisions` tests on hand-built graphs (no registry) asserting
      the `PlanDecisions` bundle: `order`, `cable_fused`, `producer_port_cycle`
      (keyed by slice position), `buf_alloc` slot regions, and `decisions`
      (install/update) for: a linear chain, a feedback loop through a
      multi-output module, and a replan (surviving + new + removed nodes).
- [ ] A minimal test registry + `build_patch` tests asserting `ExecutionPlan`
      shape for a small known graph:
  - [ ] `slots` / `active_indices` cover every node; `pool_index` stable for
        survivors across a replan.
  - [ ] `InputPort`/`OutputPort` objects carry the correct `cable_idx`,
        `connected`, and `fused` (cross-check against `cable_fused`).
  - [ ] `tombstones` lists exactly the removed nodes' pool slots on replan.
  - [ ] `param_frames` / `parameter_updates` populated only for changed params;
        `new_modules` only for installs.
  - [ ] `to_zero` / `to_zero_poly` consistent with the buffer allocation.
- [ ] `just inner -p patches-planner` green; `cargo clippy` clean.

## Notes

Part of epic **E160** (ADR 0081), phase P0. Pairs with 0975. Assert on the
*observable plan output*, not internal sequencing, so the tests survive the
action-phase split (0978) unchanged. The minimal test registry introduced here is
reused by 0978 when unit-testing the pure transforms.
