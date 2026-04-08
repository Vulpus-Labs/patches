# Architecture overview

```
  .patches file
       │
       ▼
  patches-dsl          PEG parser → FlatPatch (modules + edges, no type knowledge)
       │
       ▼
  patches-interpreter  Validates FlatPatch; resolves module types → ModuleGraph
       │
       ▼
  patches-core         Planner: ModuleGraph → ExecutionPlan
  (Planner)            Reuses existing module instances by InstanceId
       │
       ▼
  patches-engine       PatchEngine: sends ExecutionPlan to audio thread
  (PatchEngine)        via rtrb ring buffer (lock-free)
       │
       ▼
  AudioCallback        Tick loop: swaps in new plans, calls ExecutionPlan::tick()
  (audio thread)       per sample; no allocation, no blocking
```

## Crate responsibilities

| Crate | Role |
|---|---|
| `patches-dsl` | PEG parser and template expander. No knowledge of module types. |
| `patches-interpreter` | Resolves module names against the registry; builds `ModuleGraph`. |
| `patches-core` | Core traits (`Module`, `CablePool`), `ExecutionPlan`, `Planner`, `ModuleGraph`. |
| `patches-modules` | Concrete module implementations. |
| `patches-engine` | CPAL integration, `PatchEngine`, `SoundEngine`, `AudioCallback`. |
| `patches-player` | Binary: glues DSL pipeline to engine; watches file for changes. |

## Key data structures

**`ModuleGraph`** — a directed graph of module instances and typed edges with scale
factors. Built by the interpreter; consumed by the Planner.

**`ExecutionPlan`** — a flat, ordered list of module slots backed by a dense buffer
pool. The audio thread owns the active plan. Plans are immutable once handed off.

**`CablePool`** — a ping-pong pool of `CableValue` buffers. The Module `process`
method receives a `&mut CablePool` and reads inputs / writes outputs through it.

**`ModuleInstanceRegistry`** — keyed by `InstanceId`; returned by
`ExecutionPlan::into_registry()` when a plan is evicted, allowing the Planner to
reuse live module instances on the next build.
