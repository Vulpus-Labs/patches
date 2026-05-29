---
id: "0982"
title: Single-build planner invariants under proptest
priority: medium
created: 2026-05-29
closed: 2026-05-29
---

## Done

All eight properties added to [patches-planner/tests/properties.rs](../../patches-planner/tests/properties.rs)
under a dedicated `proptest!` block with `ProptestConfig { cases: 64, .. }`:

- `buffer_slot_uniqueness` — `output_buf.values()` distinct.
- `module_pool_slot_uniqueness` — `ExecutionPlan::slots[].pool_index` distinct.
- `fused_implies_forward` — fused edges strictly forward in `topology.order`.
- `scc_fusion_equivalence` — independent `tarjan_scc` run agrees with
  `cable_fused` edge-by-edge.
- `scratch_implies_all_consumers_fused` — every edge reading a slot in
  `[RESERVED_SLOTS, SCRATCH_CAPACITY)` is itself fused (the 0974 invariant).
- `slice_position_single_source` — `ports.out_port_pos[i]` equals
  `descriptor.output_position(out_name, out_idx)`.
- `producer_port_cycle_exhaustive` — `producer_port_cycle.keys()` set-equals
  `{ (edge.from, out_port_pos[i]) | every edge i }`.
- `region_containment` — every `output_buf` value in `[RESERVED_SLOTS,
  SCRATCH_CAPACITY) ∪ [SCRATCH_CAPACITY, SCRATCH_CAPACITY + CYCLE_CAPACITY)`.

Runtime: ~0.13 s for the full properties target (10 properties total
including the two 0981 smoke properties). `just push` green (build 1.75s
/ test 31.5s / clippy 7.75s).

## Summary

Assert the planner's load-bearing single-build invariants over arbitrary
graphs from the 0981 generator. Each property is a universal statement; on
failure, proptest shrinks to a minimal counter-example. These properties
cover the rules that are currently checked only on curated example graphs.

## Acceptance criteria

Each acceptance item is a separate `#[test]` property with a focused failure
message. Resist combining multiple invariants in one property — isolation
makes regressions attributable.

- [ ] **Buffer-slot uniqueness:** `output_buf.values()` are all distinct.
- [ ] **Module-pool-slot uniqueness:** `module_alloc.pool_map.values()` are
      all distinct.
- [ ] **Fused ⇒ forward:** for every edge `i` with `cable_fused[i] == true`,
      `order.position(from) < order.position(to)`. The ADR 0072 phase 1
      invariant, currently asserted only via the negative `should_panic`-style
      test on a hand-built broken classification.
- [ ] **SCC ↔ fusion equivalence:** for every edge `i`, `cable_fused[i] ==
      (scc_of[from] != scc_of[to])`.
- [ ] **Scratch ⇒ all consumers fused (0974):** for every edge whose producer
      port sits in a scratch slot, `cable_fused[edge] == true`. Structurally
      enforced — but the property version is the safety net if a future
      refactor breaks the enforcement.
- [ ] **Slice-position single source:** for every edge `i`,
      `ports.out_port_pos[i] == descriptor.output_position(edge[i].out_name,
      edge[i].out_idx)`. The frozen cache must always equal the canonical
      method's result.
- [ ] **`producer_port_cycle` is exhaustive over produced edges:** the map's
      key set equals `{ (edge.from, out_port_pos[i]) for every edge i }` (no
      stale or missing entries).
- [ ] **Region containment:** every value `v` in `plan.output_buf` satisfies
      `v ∈ [RESERVED_SLOTS, SCRATCH_CAPACITY) ∪ [SCRATCH_CAPACITY,
      SCRATCH_CAPACITY + CYCLE_CAPACITY)`.
- [ ] Default proptest case count tuned so all eight properties pass in under
      10 s total wall-clock (`cargo test`).
- [ ] `just push` green.

## Notes

Part of epic **E161**, phase P1. Depends on 0981 (generators).

Each property gets its own `proptest!` block with a single `prop_assert!` (or
`prop_assert_eq!`) so the shrinker can attribute failures. proptest's default
`Config::cases` is 256 — likely too many here given `build_patch`'s cost on a
10-node graph. Tune downward (try 64) and confirm wall-clock against the
acceptance budget.

The slice-position-single-source property is the most direct generalisation of
the 0974 fix: if anyone re-introduces a parallel derivation of the fact at any
call site, this property fires on the first graph that triggers the
divergence.
