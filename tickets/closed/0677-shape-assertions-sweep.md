---
id: "0677"
title: Replace bare `.is_ok()` with shape assertions
priority: medium
created: 2026-04-24
epic: E116
---

## Summary

Several success-path tests assert only `.is_ok()` or "did not panic"
after building a patch or connecting a graph. A regression that
returns `Ok` with a broken graph (missing nodes, wrong parameters,
wrong edge count) would pass. Sweep the flagged sites and replace
with assertions on the result shape.

## Acceptance criteria

- [ ] `patches-interpreter/src/tests/happy_path.rs:26,42,58` — assert
      on resulting `graph` node count, parameter values, and expected
      module types.
- [ ] `patches-engine/tests/planner.rs:164` — assert on returned
      `ExecutionPlan` (module count, connectivity). Remove the
      disjunction assertion at `:153` (`A || B` passes on either).
- [ ] `patches-engine/tests/builder/graph_build.rs:21,37` — 100-tick
      loops should inspect at least one cable's output range, not
      just "no panic".
- [ ] `patches-core/src/graphs/graph/tests.rs:70,152,163` —
      post-connect, assert on edge presence and fan-out counts.
- [ ] `patches-host/tests/host.rs:36-80` — assert on graph module
      types and endpoint kinds, not just non-empty.
- [ ] `patches-integration-tests/tests/poly_cables.rs:268` — assert
      on recorded samples, not just `is_some()`.
- [ ] `patches-integration-tests/tests/planner_v2.rs:95` — assert on
      specific dedup behaviour (which modules were kept, which were
      tombstoned), not just `is_empty()`.

## Notes

Mechanical sweep — low risk, high leverage. Helper functions for
"graph has module X with param Y=Z" probably worth extracting into
the test-support module once two or more tests share the pattern.
