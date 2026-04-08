---
id: "0149"
title: Reduce unnecessary cloning in planner parameter diff
epic: E025
priority: medium
created: 2026-03-20
---

## Summary

In `patches-core/src/graphs/planner/mod.rs` (lines 176-181), the parameter diffing loop clones both keys and values for every changed parameter, even when the value object is large or the map is dense. For large patches rebuilt frequently (e.g. on every hot-reload), this accumulates unnecessary short-lived allocations on the control thread.

```rust
let param_diff: ParameterMap = node
    .parameter_map
    .iter()
    .filter(|(k, v)| prev_ns.parameter_map.get_by_key(k) != Some(v))
    .map(|(k, v)| (k.clone(), v.clone()))
    .collect();
```

## Acceptance criteria

- [ ] Investigate whether `ParameterKey` can be made `Copy` or cheaply shared (e.g. `Arc<str>` key). If so, eliminate the key clone.
- [ ] Investigate whether `ParameterValue` should use `Arc` internally for large variants (e.g. string values) to make cloning O(1).
- [ ] The diff loop allocates only for entries that actually changed, not for the entire map.
- [ ] No regression in planner unit tests; add a benchmark or comment documenting expected allocation behaviour if measurement is practical.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

The fix should be considered together with T-0150 (ParameterMap API consolidation) since both touch the same types. Coordinate to avoid duplicate refactoring passes.
