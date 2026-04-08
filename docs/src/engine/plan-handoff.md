# Plan handoff & hot-reload

## The Planner

`Planner` is a stateless struct. Given a `ModuleGraph` and an optional previous
`ModuleInstanceRegistry`, it produces a new `ExecutionPlan`:

```
Planner::build(graph, prev_registry) → ExecutionPlan
```

The Planner matches modules in the new graph against the previous registry by
`InstanceId`. Matched modules are moved into the new plan with their state intact.
Unmatched modules (new in this graph) are freshly instantiated and initialised.

## Sending the plan to the audio thread

`PatchEngine` holds:
- a `SoundEngine` (owns the audio thread via CPAL)
- a reference to the rtrb ring buffer producer
- a `held_plan` slot for retry if the buffer is full

When a new plan is ready:

1. `PatchEngine` sends it via the ring buffer producer.
2. If the buffer is full, the plan is stored in `held_plan` and retried on the
   next hot-reload cycle.

## Receiving on the audio thread

The `AudioCallback::receive_plan` function runs at the top of each audio tick:

1. Poll the ring buffer for a new plan.
2. If a plan is found, `mem::replace` the active plan with the new one.
3. Push a `CleanupAction::DropPlan(old_plan)` to the cleanup ring buffer.
4. Apply parameter updates and connectivity notifications from the new plan.

The old plan is never dropped on the audio thread.

## State freshness trade-off

There is a window between when the Planner reads the old registry and when the
audio thread installs the new plan. Module state (e.g. oscillator phase) that
advances during this window is not reflected in the Planner's snapshot. This is
an intentional trade-off documented in `adr/0003-planner-state-freshness.md`.
