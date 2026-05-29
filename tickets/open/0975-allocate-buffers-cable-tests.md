---
id: "0975"
title: Lock-in unit tests for allocate_buffers cable-buffer logic
priority: high
created: 2026-05-29
---

## Summary

`allocate_buffers` (patches-planner/src/state/alloc.rs) is the transformation
that assigns each producer output port a cable buffer slot in either the
**scratch** region (`[RESERVED_SLOTS, SCRATCH_CAPACITY)`, single slot, all
consumers fused) or the **cycle** region (`[SCRATCH_CAPACITY, …)`, ping-pong
pair, stable across replans). It is the core of the ADR 0072 layout and the
stage ticket 0974 mis-keyed — yet it has **zero direct unit tests** (only
`ModuleAllocState::diff`, the module-pool sibling, is tested). Add direct tests
now, before the E160 refactor, so the restructure has a behaviour lock-in.

This is a test-only ticket: no production logic changes.

## Acceptance criteria

- [ ] Direct unit tests for `allocate_buffers` covering:
  - [ ] Scratch assignment: a producer port whose every consumer is fused gets a
        scratch slot in `[RESERVED_SLOTS, SCRATCH_CAPACITY)`, packed densely in
        forward-sweep order.
  - [ ] Cycle assignment: a producer port with ≥1 non-fused consumer gets a
        cycle slot `>= SCRATCH_CAPACITY`.
  - [ ] Cycle-slot stability across replans: a surviving `(NodeId, port_pos)`
        keeps its cycle slot; vacated slots return to `cycle_freelist` (LIFO).
  - [ ] Region flips both directions: scratch→cycle (old scratch abandoned, new
        cycle zeroed) and cycle→scratch (old cycle slot freelisted + zeroed).
  - [ ] `to_zero` reconciliation: vacated previous slots appear in `to_zero`
        without double-freeing cycle logicals.
  - [ ] Capacity edges: scratch exhaustion at `min(pool_capacity,
        SCRATCH_CAPACITY)` and cycle exhaustion at `CYCLE_CAPACITY` each return
        `PlanError::BufferPoolExhausted`.
  - [ ] Multi-output-group node: ports at slice positions 0/1/2 (declared index
        all 0) each get distinct slots keyed by slice position (0974 shape).
- [ ] Tests build inputs from hand-made descriptors + a `producer_port_cycle`
      map directly (no registry, no engine).
- [ ] `just inner -p patches-planner` green; `cargo clippy -p patches-planner`
      clean.

## Notes

Part of epic **E160** (ADR 0081), phase P0. Pairs with 0976. These lock-in tests
must pass unchanged after the IR refactor (0977) and the validation conversion
(0979), so keep them asserting on observable allocation results
(`output_buf` keys → slot region, `to_zero`, freelist/hwm), not on internal
control flow.
