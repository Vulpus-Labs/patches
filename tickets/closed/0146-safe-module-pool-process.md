---
id: "0146"
title: Replace unwrap() in ModulePool::process
epic: E024
priority: high
created: 2026-03-20
---

## Summary

`ModulePool::process()` in `patches-engine/src/pool.rs` (line 85) calls `.as_mut().unwrap()` on a module slot. If the execution plan references a slot index that is unoccupied — which should be prevented by the planner invariant that plan indices match pool slots — this panics on the audio thread.

```rust
let m = self.modules[idx].as_mut().unwrap();
```

## Acceptance criteria

- [ ] In debug/test builds, the missing-slot case is caught with `debug_assert!` and a descriptive message identifying the slot index.
- [ ] In release builds, a missing slot is a silent no-op: the module's process step is skipped for that tick.
- [ ] Add a unit test that constructs a `ModulePool` with a gap in its populated slots and calls `process` on the missing index; assert it does not panic and produces no output.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

The planner's guarantee ("plan indices always match pool occupancy") is currently maintained by convention, not enforcement. This ticket makes the audio callback robust against planner bugs rather than amplifying them into process crashes.
