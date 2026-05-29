---
id: "0977"
title: Introduce typed IR bundles; rewire make_decisions to thread them
priority: medium
created: 2026-05-29
---

## Summary

Replace the bare tuples and loose maps that the decision-phase sub-stages
exchange with **named, frozen IR bundles**, so each stage becomes
`fn(IR_prev) -> Result<IR_next, PlanError>` and is independently constructible in
tests. No logic change — same plans out, goldens bit-identical.

Apply the ADR 0081 **no-churn rule**: each bundle is finalised once and passed
forward frozen; later IRs **compose** the earlier bundles they still need rather
than re-mapping fields or re-deriving facts. Discard a bundle / narrow the
interface where later stages don't need it (e.g. drop the raw `SccPartition`
once `order` + `cable_fused` are derived, if unused downstream).

## Acceptance criteria

- [ ] Introduce IR types (final names TBD): `Topology { order, cable_fused,
      fas_size }`, `PortClassification { out_port_pos, producer_port_cycle }`,
      `BufferLayout` (embeds `BufferAllocation` + `ModuleAllocDiff` + resolved
      input buffers). Keep `GraphIndex` and the `NodeDecision` set as-is.
- [ ] Each stage signature becomes `fn(IR_prev, …) -> Result<IR_next, PlanError>`;
      `make_decisions` becomes a thin composition of these stages.
- [ ] `PortClassification` is the **single** owner of `out_port_pos` and
      `producer_port_cycle`; `allocate_buffers`, the scratch/fused validation,
      and the action phase all read from it — none re-derive. Grep audit
      confirms one derivation site per fact.
- [ ] No bundle re-maps another's fields; later IRs embed frozen prior bundles.
      Where a field is dropped, a comment notes it is intentionally narrowed.
- [ ] Per-stage direct unit tests: build `IR_prev` by hand, assert `IR_next`
      (extends the 0975/0976 lock-in tests to the new boundaries).
- [ ] `PlanDecisions` either becomes a thin composition of the frozen bundles or
      is removed in favour of returning them; the builder destructures the new
      shape.
- [ ] Audio goldens bit-identical; `just push` green.

## Notes

Part of epic **E160** (ADR 0081), phase P1. Depends on 0975 + 0976 (lock-in
safety net). Pure structural change — if any golden shifts, the refactor changed
behaviour and must be corrected, not regenerated. Open question (ADR 0081 q.1):
borrowed vs owned IRs given `GraphIndex<'a>`; default to keeping the existing
borrow lifetimes unless the action-phase split (0978) forces owning.
