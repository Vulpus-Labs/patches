---
id: "0150"
title: Consolidate ParameterMap lookup API
epic: E025
priority: medium
created: 2026-03-20
---

## Summary

`ParameterMap` in `patches-core/src/modules/parameter_map.rs` exposes three overlapping lookup paths:

- `get(name: &str)` — implicitly uses index 0
- `get_param(name: &str, index: usize)` — explicit index
- `get_by_key(key: &ParameterKey)` — pre-constructed key

Callers must know which variant is appropriate. The implicit-index-0 path (`get`) is a footgun for indexed parameters, and the dual name vs. key access adds surface area with no clear guidance on when to prefer one over the other.

## Acceptance criteria

- [ ] Audit all call sites for `get`, `get_param`, and `get_by_key` across the workspace.
- [ ] Settle on a single primary lookup form (suggested: `get(name: &str, index: usize)` with `index: 0` being the common case, possibly via a default-argument workaround or separate `get_scalar(name)` alias).
- [ ] Deprecate or remove the redundant variants, updating all call sites.
- [ ] Document the chosen API with a doc comment explaining when `index > 0` applies.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

Coordinate with T-0149, which touches the same `ParameterKey`/`ParameterValue` types.
