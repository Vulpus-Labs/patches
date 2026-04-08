---
id: "0183"
title: Implement TimingShim wrapping Box<dyn Module>
priority: medium
created: 2026-03-24
epic: E033
depends: "0182"
---

## Summary

Implement `TimingShim` in `patches-profiling/src/timing_shim.rs`. It wraps a
`Box<dyn Module>`, delegates every `Module` method to the inner module, and
times `process()` and `periodic_update()` calls, recording results into a
shared `TimingCollector`.

## Acceptance criteria

- [ ] `src/lib.rs` declares `pub mod timing_shim`
- [ ] `TimingShim` holds `inner: Box<dyn Module>`, `collector: Arc<TimingCollector>`, and `name: &'static str` (captured from the inner descriptor at construction)
- [ ] `TimingShim` fully implements the `Module` trait; all methods except `process` are pure delegation to `inner`
- [ ] `process()` wraps the inner call with `Instant::now()` and calls `collector.record_process()`
- [ ] `TimingShim` implements `PeriodicUpdate`: `periodic_update()` delegates to `inner.as_periodic().unwrap()` with `Instant` timing and calls `collector.record_periodic()`
- [ ] `TimingShim::as_periodic()` returns `Some(self)` iff `inner.as_periodic().is_some()`; otherwise `None`
- [ ] `TimingShim::as_midi_receiver()` delegates to `inner.as_midi_receiver()` with no timing
- [ ] `cargo clippy -p patches-profiling` clean

## Notes

The `as_periodic` / double-borrow pattern: checking `self.inner.as_periodic().is_some()` in the guard compiles cleanly under NLL because the borrow ends before `Some(self)` is returned.

`TimingShim::instance_id()` must delegate to `inner.instance_id()` so the planner's `InstanceId`-based tracking continues to work correctly if the plan is ever inspected after profiling.
