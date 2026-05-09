---
id: "0848"
title: Fusion phase 1 — SCC, topo-sort, and cable fused/cyclic flagging in planner
priority: medium
created: 2026-05-09
epic: E141
adr: 0072
---

## Summary

Add graph analysis to the planner so each cable in the
`ExecutionPlan` is annotated as either **fused** (inside an acyclic
region; consumer can read producer's current-tick output) or
**cyclic** (sits on a feedback arc; must retain the 1-sample delay).
The annotation is emitted but **the engine continues to ignore it**.
This phase delivers the planner machinery in isolation, testable
without any engine change, with bit-identical audio output to today.

The current `compute_order` in `patches-planner/src/state/mod.rs`
sorts module ids alphabetically. Replace with a topo-sort over the
condensation of the SCC graph. Within an SCC, ordering is arbitrary
for non-trivial SCCs and forced for trivial ones; the condensation's
topo-sort fixes inter-SCC ordering. The `active_indices` array
respects the new ordering.

## Acceptance criteria

- [ ] Tarjan's SCC implemented over the module dependency graph
      (edge `A → B` for each cable `A.out → B.in`). Returns the
      list of SCCs in reverse topological order.
- [ ] Condensation topo-sort produces a deterministic
      `active_indices` ordering. For trivial SCCs (single module,
      no self-loop) the order matches inter-SCC topo. For
      non-trivial SCCs, an internal order is chosen deterministically
      (e.g. alphabetical within the SCC).
- [ ] Each cable in the plan is annotated `fused: bool`. A cable
      `A.out → B.in` is `fused` iff `A` and `B` are in different
      SCCs *and* `A` precedes `B` in `active_indices`. Cables
      within a non-trivial SCC are `fused: false`.
- [ ] Validation at plan build time: for every cable with
      `fused: true`, producer index < consumer index in
      `active_indices`. Violation is a planner bug, panics with a
      descriptive message.
- [ ] Engine reads continue to use the existing read-from-`1 - wi`
      path on every cable, regardless of the new flag. No
      `CablePool` changes.
- [ ] `just inner -p patches-planner` passes; new SCC and topo-sort
      tests cover: empty graph, single module, simple cycle, nested
      cycles, disjoint subgraphs, mixed acyclic + cyclic.
- [ ] All existing patch-output tests produce **bit-identical**
      audio output to before this ticket. (Engine ignores the new
      flag, so audio behaviour must be unchanged.)
- [ ] FAS size (count of `fused: false` cables outside trivial SCCs)
      is reported on plan build for the test corpus, to validate the
      assumption that typical patches have a small FAS.

## Notes

This ticket is pure analysis. The flag travels through the plan as
metadata and is consumed in 0849. Until 0849 lands the flag is
inert — keep its representation cheap to avoid carrying overhead
that is never used.

The validation invariant in step 4 is the load-bearing safety check
for phase 2: if it ever fails after this ticket, phase 2 would read
stale data. Lock it down with property tests: generate random DAGs,
condense, topo-sort, assert the invariant.

Diagnostic surface (LSP "this cable is in a feedback loop" inlay
hint) is enabled by this ticket but not implemented here — defer
to a future LSP ticket if/when motivated.

Tarjan's is `O(V + E)` and graphs are dozens of modules; cost is
negligible. No need for incremental SCC update across replans —
recompute from scratch on every plan build.
