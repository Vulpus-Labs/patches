---
id: "0513"
title: Extract patches-planner crate and move PlannerState
priority: high
created: 2026-04-17
---

## Summary

Move the planner, `ExecutionPlan`, and `PlannerState` out of
`patches-engine` and `patches-core` into a new `patches-planner`
crate. `ModuleGraph` stays in `patches-core` as a foundational
topology type.

Part of epic E089 (see ADR 0040). Depends on 0512.

## Acceptance criteria

- [ ] New `patches-planner/` crate exists with `publish = false`.
- [ ] Moved from `patches-engine`: `src/planner.rs`, the
  `ExecutionPlan`-side of `src/builder/` (plan construction,
  parameter/port/module/tombstone diffs). The kernel-facing
  wiring that consumes the plan stays in `patches-engine`.
- [ ] Moved from `patches-core`: `src/graphs/planner/`
  (`PlannerState`, `graph_index`, `tests`, decision classification).
- [ ] `patches-core/src/graphs/graph/` (`ModuleGraph`) stays in place.
- [ ] `patches-planner` depends on `patches-core` and
  `patches-registry`; it does not depend on `patches-engine`.
- [ ] `patches-engine` depends on `patches-planner` (to consume
  `ExecutionPlan`).
- [ ] Consumers updated: `patches-engine`, `patches-clap`,
  `patches-player`, `patches-integration-tests`.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

ADR 0012 (planner v2 graph-diffing, status *proposed*) overlaps with
this move. Before starting, decide whether to land ADR 0012's design
first, land it as part of this ticket, or deliberately carve the
current code and revisit ADR 0012 afterwards inside the new crate. The
ticket deliberately does not prescribe — check the ADR's status and
the latest `PlannerState` code before committing.

`ExecutionPlan` currently lives at
`patches-engine/src/builder/mod.rs:110+`. Its consumers are
`engine.rs`, `kernel.rs`, `execution_state.rs`, `callback.rs`,
`processor.rs`, `patches-clap/src/plugin.rs`,
`patches-integration-tests/tests/planner_v2.rs`. All update to import
from `patches-planner`.

Confirm dep graph with `cargo tree -p patches-engine` after the move:
`patches-planner` should appear; there should be no cycle.
