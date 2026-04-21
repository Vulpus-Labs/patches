---
id: "0595"
title: Plumb `ParamFrame` through `ExecutionPlan` adoption; build `ParamView` on the audio thread
priority: high
created: 2026-04-20
---

## Summary

Carry a per-instance `ParamFrame` (from Spike 3,
`patches-ffi-common::param_frame`) through the plan-adoption
channel (ADR 0002) so the audio thread can construct a
`ParamView<'_>` without touching `ParameterMap`.

## Scope

- Extend `ExecutionPlan` (or its per-instance entry) with a
  `ParamFrame` slot plus the `ParamLayout` + `ParamViewIndex`
  needed to read it. Layout + index are built once at `prepare`
  and reused across plans for the life of the instance.
- In the planner / builder, replace the current
  `ParameterMap`-per-instance stash with `pack_into(layout, map,
  &mut frame)` on the control thread. The resulting frame ships
  inside the plan.
- On the audio thread, in `adopt_plan`, construct
  `ParamView::new(&layout, &frame, &index)` and keep it available
  to the module's existing update call site. Don't change the
  trait signature yet — ticket 0596 does that.
- Frame ownership follows the existing plan ownership rules:
  evicted plans drop on the cleanup worker (ADR 0010).

## Acceptance criteria

- [ ] `ExecutionPlan` carries one `ParamFrame` + layout handle
      per live module instance.
- [ ] Control-thread frame construction is allocation-free after
      the frame is sized at `prepare`.
- [ ] Audio thread does not allocate when constructing the view
      or reading from it (validated in follow-up ticket 0600 under
      the allocator trap).
- [ ] Existing trait signature (`&ParameterMap`) unchanged; this
      ticket is plumbing only.
- [ ] Full workspace tests green.

## Non-goals

- Flipping the trait signature (0596).
- Migrating module implementations (0597, 0598).
- Removing the shadow oracle (0600).
