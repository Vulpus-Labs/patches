# Off-thread deallocation

Dropping a `Box<dyn Module>` or an `ExecutionPlan` executes arbitrary destructor
code, which may allocate or block. This is incompatible with audio-thread
constraints. Patches routes all deallocation to a dedicated background thread.

## The cleanup thread

A thread named `"patches-cleanup"` is started alongside the audio stream. It
owns the consumer end of a lock-free ring buffer. The audio thread pushes
`CleanupAction` values onto this ring buffer; the cleanup thread drains and drops
them.

```rust
enum CleanupAction {
    DropModule(Box<dyn Module>),
    DropPlan(ExecutionPlan),
}
```

## When modules are cleaned up

- **Tombstoned modules** — when the Planner removes a module that existed in the
  previous plan, `ModulePool::tombstone` extracts it and the audio callback pushes
  it as `CleanupAction::DropModule`.
- **Evicted plans** — when a new plan is installed, the old plan is extracted via
  `mem::replace` and pushed as `CleanupAction::DropPlan`.

## Fallback

If the cleanup ring buffer is full, the audio callback falls back to dropping
inline (with an `eprintln` warning). This is a last resort and should not occur
in normal operation.

## Testing

`HeadlessEngine` (in `patches-integration-tests`) mirrors the full plan-swap
sequence including the cleanup thread. Its `stop()` method joins the cleanup
thread, guaranteeing all deallocation has completed before the test asserts.
