---
id: "0159"
title: Document PortConnectivity duplication rationale and DEFAULT_MODULE_POOL_CAPACITY
epic: E026
priority: low
created: 2026-03-20
---

## Summary

Two underdocumented design choices identified in the sniff-test review:

1. **`PortConnectivity` duplication.** `PortConnectivity` in `patches-core/src/modules/module.rs` (lines 102-126) tracks connected/disconnected state per port, duplicating the `connected: bool` field already present on `MonoInput`/`PolyInput`/etc. The duplication is intentional (efficient change detection for the planner diff path, T-0080), but there's no note at the type level explaining this.

2. **`DEFAULT_MODULE_POOL_CAPACITY = 1024`** in `patches-engine/src/engine.rs` (line 19) is a magic number with no comment explaining how it was chosen or what scale of patches it supports.

## Acceptance criteria

- [ ] A doc comment on `PortConnectivity` (or adjacent) explains why it exists alongside the per-port `connected` field: it provides a snapshot of connectivity at plan-build time for efficient diffing without re-inspecting individual port fields.
- [ ] A comment on `DEFAULT_MODULE_POOL_CAPACITY` states the intended upper bound for typical patches (e.g. "supports patches with up to ~1000 simultaneous module instances; increase if needed") and any reasoning behind the specific value.
- [ ] No code changes — documentation only.
- [ ] `cargo clippy` clean; `cargo test` passes.
