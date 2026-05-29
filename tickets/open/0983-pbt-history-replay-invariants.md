---
id: "0983"
title: History-replay planner invariants under proptest
priority: medium
created: 2026-05-29
---

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
