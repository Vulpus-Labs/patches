---
id: "0592"
title: Audio-thread tagging API (mark_audio_thread, per-thread TLS)
priority: high
created: 2026-04-20
---

## Summary

Expose a stable, feature-agnostic tagging API from
`patches-alloc-trap`:

```rust
pub fn mark_audio_thread();
pub fn is_audio_thread() -> bool;
```

Called once from each audio-thread entry point. When the
`audio-thread-allocator-trap` feature is off, both functions are
inline no-ops. When the feature is on, `mark_audio_thread()`
sets a thread-local flag and latches `TRAP_ARMED = true` so the
`TrappingAllocator` (ticket 0591) begins aborting on
allocations performed by this thread.

## Acceptance criteria

- [ ] `mark_audio_thread()` is idempotent — calling it twice on
      the same thread is a no-op the second time.
- [ ] `is_audio_thread()` returns the current flag state; used
      by tests.
- [ ] With feature off: both functions are
      `#[inline(always)] fn … {}` / `false` and compile to
      nothing.
- [ ] With feature on: flag lives in a `thread_local! {
      static AUDIO_THREAD: Cell<bool> }` inside the crate.
      `mark_audio_thread()` sets it true and stores
      `TRAP_ARMED = true` with `Ordering::Release`.
- [ ] `TrappingAllocator` (from ticket 0591) reads the flag
      via the same TLS; the allocator aborts iff both
      `TRAP_ARMED.load(Acquire) == true` and
      `AUDIO_THREAD.get() == true`.
- [ ] `NoAllocGuard::enter()` is re-implemented on top of
      `mark_audio_thread()` + a drop that clears the flag, so
      test-scope use and production tagging share one code
      path.
- [ ] Unit tests in the crate:
      - `is_audio_thread()` reports false on a fresh thread.
      - After `mark_audio_thread()`, flag is true; flag is
        per-thread (a second thread reads false).
      - Scope guard sets and clears as expected.
- [ ] Clippy clean.

## Notes

The thread-tag approach replaces the "arm around the hot path"
scope guard used by the existing alloc_trap integration test.
For real audio threads, one call at callback-thread startup is
enough — we never want the trap to drop for that thread.
Test scopes that drive `HeadlessEngine::tick` from the main
test thread still use the guard.

Depends on 0591.
