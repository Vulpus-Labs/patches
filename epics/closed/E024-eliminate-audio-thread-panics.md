---
id: "E024"
title: Eliminate panics in and near the audio thread
status: closed
priority: high
created: 2026-03-20
tickets:
  - "0145"
  - "0146"
  - "0147"
  - "0148"
---

## Summary

Several code paths panic on invalid invariants rather than handling them gracefully. Two of these paths are reachable from the audio thread (`unreachable!()` in `cables.rs`/`cable_pool.rs` and `unwrap()` in `pool.rs`); a bug in the planner or a malformed hot-reload could trigger them and cause a hard audio dropout or process crash. The DSL parser and `graph_to_yaml` have similar issues on the control thread: malformed input or a refactor-induced stale assumption causes a panic instead of a recoverable error.

## Tickets

- [T-0145](../tickets/open/0145-safe-hot-path-cable-reads.md) — Replace `unreachable!()` in cable hot path with safe fallbacks
- [T-0146](../tickets/open/0146-safe-module-pool-process.md) — Replace `unwrap()` in `ModulePool::process`
- [T-0147](../tickets/open/0147-dsl-parser-error-propagation.md) — Replace parser `unwrap()` calls with proper error propagation
- [T-0148](../tickets/open/0148-graph-yaml-safe-node-lookup.md) — Eliminate `unwrap()` in `graph_to_yaml` node lookup
