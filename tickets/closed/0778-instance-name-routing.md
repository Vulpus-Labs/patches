---
id: "0778"
title: Route instance names and types from planner to observer with drop ladder
priority: medium
created: 2026-05-01
---

## Summary

Slot index → (instance `QName`, module type name) mapping is needed
by the CPU monitor observer to label per-instance cost estimates and
to aggregate by module type. Slot indices are assigned by the
control thread in `ModuleAllocState::diff`; both instance names and
type names live in the planner's ephemeral `nodes` map but do not
flow into `ExecutionPlan` or `ModulePool`. Build slot-indexed
`Vec<QName>` and `Vec<&'static str>` alongside each plan and route
them through the audio thread to the monitor observer with a drop
ladder, so audio-thread work is zero on the default disabled path.

See [ADR 0065](../../adr/0065-per-instance-cpu-monitoring.md) for
the full design.

## Acceptance criteria

- [ ] Builder constructs `Option<PlanMeta { names: Vec<QName>,
  types: Vec<&'static str> }>` parallel to plan, both keyed by slot
  idx (covers active set; periodic subset reuses same vecs via
  `periodic_indices`).
- [ ] `PatchProcessor::adopt_plan` signature extended to accept
  `meta: Option<PlanMeta>` as a separate argument (not stored on
  `ExecutionPlan`).
- [ ] On adopt, `meta` is pushed to the monitor SPSC channel if a
  monitor is configured.
- [ ] Drop ladder: SPSC full → plan cleanup channel; cleanup full →
  drop in-thread (last resort, expected unreachable).
- [ ] Builder passes `None` when monitor disabled; no `Vec`
  allocation, no traversal.
- [ ] Observer rebuilds slot-indexed name table on `PlanMeta`
  receipt; clears prior table.
- [ ] Tests: name vector matches slot order across hot reloads
  (including renames, removals, additions).

## Notes

- `QName` likely uses `Rc<str>` or `Arc<str>` internally. If `Rc`,
  ensure cross-thread send is sound (it is not for `Rc`); switch to
  `Arc` or clone strings into a thread-safe form before send.
- This ticket creates the transport but does not produce monitor
  data. Ticket 0779 produces the data; ticket 0780 consumes it for
  UI.
