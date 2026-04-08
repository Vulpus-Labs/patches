---
id: "0148"
title: Eliminate unwrap() in graph_to_yaml node lookup
epic: E024
priority: high
created: 2026-03-20
---

## Summary

`graph_to_yaml()` in `patches-core/src/graph_yaml.rs` (line 184) calls `graph.get_node(id).unwrap()` where `id` is drawn from `node_ids()`. The invariant "an id from `node_ids()` must be present in the graph" is locally sound but fragile: an intermediate refactor that introduces a filter or transforms the id set before lookup could silently break it.

## Acceptance criteria

- [ ] Replace the `unwrap()` with `ok_or_else(|| ...)` returning a typed error variant (e.g. `GraphYamlError::NodeMissing(id)`), propagated via `?`.
- [ ] The function signature of `graph_to_yaml` reflects the fallibility if it doesn't already.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

This is the lowest-risk issue in E024 (control thread only, invariant currently holds), but fixing it closes a future refactoring footgun and is a small, self-contained change.
