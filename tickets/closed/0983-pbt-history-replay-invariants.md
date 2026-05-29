---
id: "0983"
title: History-replay planner invariants under proptest
priority: medium
created: 2026-05-29
closed: 2026-05-29
---

## Done

Six properties added to [patches-planner/tests/properties.rs](../../patches-planner/tests/properties.rs)
under a dedicated `proptest!` block with `ProptestConfig { cases: 32, .. }`
and histories capped at 10 edits:

- `replay_holds_single_build_invariants` — drives `PatchBuilder::build_patch`
  through every prefix of the history; the eight 0982 invariants
  (factored into `check_single_build_invariants`) hold at every step.
- `cycle_slot_stability_across_replan` — consecutive plans agree on
  `cable_idx` for any `(node, slice_pos)` that is a cycle producer in
  both. Scratch slots are excluded (recomputed each plan per ADR 0072
  phase 5).
- `cycle_slot_mass_conservation` — every cycle slot ever allocated
  through the history lives in either the current cycle allocation or
  the cycle freelist at every step.
- `tombstones_match_pool_diff` — `plan.tombstones` set-equals
  `prev.pool_map.keys() \ new.pool_map.keys()` mapped through the prior
  slot index.
- `make_decisions_deterministic` — two consecutive `make_decisions` runs
  on a fixed `(graph, prev_state)` produce equal `topology`, `ports`,
  and `output_buf`. The pure decision phase is deterministic.
- `build_patch_deterministic_modulo_instance_id` — two consecutive
  `build_patch` runs produce canonically-equal plans, **with
  `pool_index` elided from the canonical form** (see below).
- `type_change_mints_fresh_instance` — after a descriptor-kind swap on
  any node, that node's `InstanceId` differs from its prior value.

## Property relaxation: pool-slot identity is non-deterministic across runs

`build_patch_deterministic_modulo_instance_id` originally compared
`slots[].pool_index` and `new_modules[].0` directly; it failed in
shrinking on a two-node graph (`PolyIO -> MultiOut`). Diagnosis:
[`ModuleAllocState::diff`](../../patches-planner/src/state/alloc.rs)
iterates the incoming `HashSet<InstanceId>` to assign pool slots to
fresh InstanceIds:

```rust
for &id in new_ids {
    if let Some(&existing) = self.pool_map.get(&id) { ... }
    else {
        let idx = if let Some(recycled) = freelist.pop() { recycled }
                  else { let i = next_hwm; next_hwm += 1; i };
        slot_map.insert(id, idx);
    }
}
```

`HashSet` iteration depends on its random `RandomState` seed, so two
consecutive builds may pair the same NodeIds with *different* fresh
InstanceIds *and* swap which of those gets pool slot 0 vs 1. The audio
thread only ever sees one build's output, so the swap is harmless in
production. Within a single build the mapping is internally consistent.

**Decision (per ticket open question):** relax the property rather than
sort `new_ids` in `diff`. The relaxation is local to the test (drop
`pool_index` from `PlanCanonical`, retain slot *shapes* — cable indices,
output buffers, fas/hwm scalars — all of which are deterministic from
`allocate_buffers`' forward sweep). If a future ADR comes to depend on
cross-run pool-slot stability (e.g. for cross-process plan replay), the
fix is in `diff`: collect `new_ids` into a `BTreeSet` or sort by
InstanceId before allocating. Captured here so a later reader does not
re-derive the diagnosis.

## Runtime

Full properties target (17 properties): ~0.13 s. `just push` green
(build 0.4 s / test 7.8 s / clippy 7.4 s). `just smoke` green.

## Summary

Drive the planner through a generated sequence of edits (an `arb_history`)
and assert the invariants that span multiple builds: cycle-slot stability
across churn, mass conservation of allocations, tombstone correctness, and
`build_draft` determinism. This is the corner-case fuzzer — the property layer
expected to find a real bug if one is present.

## Acceptance criteria

- [ ] For every `arb_history`, build the sequence step-by-step through
      `PatchBuilder::build_patch` and assert all single-build invariants from
      0982 hold at every step.
- [ ] **Cycle-slot stability:** for every consecutive `(prev, new)` plan pair,
      every `(node, slice_pos)` that is a cycle producer in both has the same
      `cable_idx`. The audio-thread feedback state contract.
- [ ] **Mass conservation:** across the full history, every buffer slot ever
      allocated ends each step in either the current `output_buf.values()` or
      the current `cycle_freelist` (no slot quietly lost between steps).
- [ ] **Tombstone correctness:** for every step, `tombstones` equals exactly
      the set of pool slots whose `InstanceId` was in the prior
      `module_alloc.pool_map` but is absent from the new one.
- [ ] **`build_draft` determinism:** for a fixed `(graph, prev_state)`, two
      consecutive `build_draft` runs produce equal `PlanDraft` field-by-field
      (after canonicalising any `HashMap` to a sorted vec — define a
      `canonicalise` helper used by every comparison).
- [ ] **Replan freshness on type change:** after an edit that changes a
      node's `module_name`, the next plan's `instance_ids[node]` differs from
      the prior build's `instance_ids[node]`.
- [ ] Default proptest case count tuned so the history-replay properties pass
      in under 20 s total wall-clock.
- [ ] `just push` + `just smoke` green.

## Notes

Part of epic **E161**, phase P2. Depends on 0981 (generators) and 0982
(single-build properties — invoked per step).

The shrinker is the win on this ticket. A 50-edit history reproducing a bug
auto-shrinks to a minimal one — typically 2–4 edits. Cap individual histories
at ≤ 20 edits so single-case wall-clock stays bounded; the case count
(proptest default 256) is reduced to keep total runtime under the budget.

If `build_draft` determinism fails, the most likely culprit is
`HashSet<InstanceId>` iteration order in `ModuleAllocState::diff` affecting
slot assignment when multiple new ids contend for the freelist. This is a
genuine observation worth capturing in the ticket close notes — either the
determinism property is too strong (and should be relaxed to "equal after
canonical reordering") or `diff` should sort its new-id iteration. Decide on
the first failure.

The replan-freshness-on-type-change property is a structural cross-check
against a class of survivor-misclassification bugs: if `classify_nodes`'
module-name guard ever weakens, a type change would silently retain the old
`InstanceId` and this property catches it.
