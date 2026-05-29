---
id: E162
title: Planner pipeline cleanup — close E160 follow-ups
status: closed
created: 2026-05-29
closed: 2026-05-29
---

## Goal

E160 / ADR 0081 restructured `patches-planner` into typed analysis / validation /
transformation stages and split the action phase into pure transforms behind a
thin impure shell. A post-landing review found the structural goals were met —
slice-position re-derivation is gone, `Topology` / `PortClassification` /
`BufferLayout` / `PlanDraft` are typed, the pure/impure split holds — but a
handful of seams reintroduce the very patterns ADR 0081 set out to remove:
the seven-arity edge tuple, an unpack-and-rebroadcast at the
decision/action boundary, a `NodeId`-keyed install-metadata side-table, and
post-build mutation of `ExecutionPlan` in `Planner::build_full`.

This epic closes those follow-ups. It is **behaviour-preserving** — same plans
out, audio goldens bit-identical, integration suite green. Pure structural
work: tighten the seams ADR 0081 already specified, do not extend the design.

## Scope

**In:**

- Replace the bare seven-tuple edge representation with a named `Edge` struct
  across the decision-phase stages and validators.
- Eliminate the `PlanDecisions` unpack-and-rebroadcast into
  `PatchBuilder::build_draft`'s 8-argument signature.
- Replace the `NodeId`-keyed `InstallMeta` / `Instantiated::modules`
  side-tables with positional, install-order-aligned collections so
  `assemble` has no missing-key error path.
- Fold tracker-receiver detection into the builder via `InstallMeta`; remove
  the post-build mutation in `Planner::build_full`.
- Promote the ad-hoc `fused_by_input` map in `build_draft` to a derived
  bundle alongside `Topology`; introduce a producer-port key newtype shared
  by `PortClassification` and `BufferAllocation`.
- Unify the allocation transformation/state type pairs
  (`BufferAllocation` ↔ `BufferAllocState`, `ModuleAllocDiff` ↔
  `ModuleAllocState`) so the end-of-`build_draft` re-pack disappears.
- Small smell cleanups: dedup `pack_into` call sites behind a helper; drop
  dead `ResolvedGraph.index` / `BufferAllocState::scratch_hwm`; inline
  single-use free helpers into their owning `impl::build` site.

**Out (deferred / other work):**

- Any behaviour change in plan content, ordering, or allocation policy — this
  epic moves no fields out of any stage; it only tightens types.
- Parallel execution / SCC thread partitioning (ADR 0072 desiderata) —
  unchanged by this work.
- `BuildError` ↔ `PlanError` boundary collapse (currently
  `FusedOrderViolation` / `ScratchFusedConflict` stringify into
  `BuildErrorKind::InternalError`). Reviewed and intentionally retained: tests
  call `make_decisions` directly for the structured variants; engine-side
  callers only need the conflated `InternalError`. Revisit only if a consumer
  needs to match externally.
- ADR 0081 open q.1 (owned vs borrowed IRs) — keep current borrow lifetimes
  unless a ticket here forces a change.

## Tickets

- [ ] [0987 — Named `Edge` struct replacing the seven-tuple](../../tickets/open/0987-edge-struct.md)
- [ ] [0988 — Pass `PlanDecisions` through `build_draft` (drop 8-arg fan-out)](../../tickets/open/0988-build-draft-arg-collapse.md)
- [ ] [0989 — Install pipeline: positional install metadata, in-order modules vec](../../tickets/open/0989-install-positional.md)
- [ ] [0990 — Builder owns tracker-receiver detection via `InstallMeta`](../../tickets/open/0990-tracker-receiver-in-builder.md)
- [ ] [0991 — Per-input fused-flag bundle; producer-port key newtype](../../tickets/open/0991-fused-by-input-bundle.md)
- [ ] [0992 — Unify allocation transformation/state types](../../tickets/open/0992-alloc-state-unify.md)
- [ ] [0993 — Smell cleanups: pack_frame helper, dead fields, inline helpers](../../tickets/open/0993-planner-smell-cleanups.md)

## Dependency order

```text
0987 ─┐
      ├─> 0988 ─┐
0991 ─┘         ├─> 0989 ─> 0990
                │
0992 ───────────┘
0993 (independent, any time)
```

0987 (Edge struct), 0991 (fused-by-input bundle, producer-port key), 0992
(allocation type unification), and 0993 (smell cleanups) are independent and
can run in parallel. 0988 builds on 0987 + 0991 + 0992 (its argument list
benefits from each of those tightenings). 0989 builds on 0988 (touches the
same draft/assemble structures). 0990 builds on 0989 (extends `InstallMeta`).

## Acceptance

- The seven-tuple edge representation is gone from planner code; named
  `Edge` fields used throughout. A grep audit shows zero
  `(NodeId, &'static str, usize, NodeId, &'static str, usize, _)` literals
  in `patches-planner/src/`.
- `PatchBuilder::build_draft` no longer carries
  `#[allow(clippy::too_many_arguments)]`; signature passes a frozen bundle
  rather than unpacked components.
- `assemble` has no `HashMap::remove(id).ok_or_else(InternalError)` path;
  installs are walked positionally and modules drained in install order.
- `Planner::build_full` does not mutate `ExecutionPlan` after the builder
  returns; tracker-receiver indices are emitted by the builder.
- `fused_by_input` is constructed once at stage-boundary, not inside
  `build_draft`. Producer-port keys are a named type, not a tuple, shared
  by `PortClassification` and `BufferAllocation`.
- End-of-`build_draft` no longer constructs sibling `BufferAllocState` /
  `ModuleAllocState` instances field-by-field from the transformation
  outputs.
- Audio goldens bit-identical; `just push` green; `just smoke` green.

## Notes

This epic is the structural follow-up to E160. None of these tickets are
behaviour-changing; they each tighten a seam the post-E160 review flagged.
Most are small (single-day) tickets — they are split for review surface
rather than for staging.
