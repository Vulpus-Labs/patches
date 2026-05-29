---
id: "0978"
title: Split action phase into pure transforms plus a thin impure shell
priority: medium
created: 2026-05-29
---

## Summary

The action phase (`PatchBuilder::build_patch_with_meta`, ~360 lines) interleaves
pure transformation (build `InputPort`/`OutputPort` objects, partition inputs,
pack param frames, assemble `ModuleSlot`s and the `ExecutionPlan`) with side
effects (`registry.create`, `module.set_ports`, `InstanceId::next()`). The pure
logic can't be unit-tested without a real registry and real modules.

Extract a pure `PlanDraft` transform — everything needed to describe the plan
**without instantiating modules** — behind a thin impure shell that performs only
the unavoidable effects. Inject the `InstanceId` source so the pure path is
deterministic under test.

## Acceptance criteria

- [ ] Define `PlanDraft` (pure) carrying, per node: resolved port objects,
      partitioned inputs, output buffers, param-frame plan, install/update intent,
      tombstones, `to_zero`/`to_zero_poly`, and the carried allocation/hwm data —
      composed from the 0977 frozen IR bundles, no re-derivation.
- [ ] Pure transform `decisions IR -> PlanDraft` with **no** registry / module /
      global-counter access; unit-tested with descriptors only (reuse the 0976
      minimal test registry's descriptors, but the transform itself takes no
      registry).
- [ ] Thin impure shell `PlanDraft + registry + id_source -> (ExecutionPlan,
      PlannerState)`: limited to `registry.create`, `set_ports`, and id minting.
- [ ] `InstanceId` source injected (allocator trait or seed) so installs get
      deterministic ids in tests; production passes the global source. (ADR 0081
      open q.3.)
- [ ] Port-object construction, input partitioning, and param-frame packing each
      have direct unit tests on `PlanDraft` inputs (no engine, no registry).
- [ ] `ffi` port-offset and structural-change checks still run; their existing
      tests pass.
- [ ] Audio goldens bit-identical; `just push` green; `just smoke` green
      (integration tests touched).

## Notes

Part of epic **E160** (ADR 0081), phase P2. Depends on 0977. This is the
load-bearing change — keep it behaviour-preserving and lean on the golden +
integration suite plus the 0975/0976 lock-in tests. May force a decision on
owned vs borrowed IRs (ADR 0081 q.1): if `PlanDraft` must outlive the borrow of
the graph, it owns the needed data.
