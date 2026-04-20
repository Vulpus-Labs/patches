---
id: "0591"
title: TrappingAllocator in shared crate behind feature flag
priority: high
created: 2026-04-20
---

## Summary

Lift the `TrappingAllocator` currently living in
`patches-integration-tests/tests/alloc_trap.rs` into a shared
crate `patches-alloc-trap` (new), gated by an
`audio-thread-allocator-trap` cargo feature. No behaviour change
yet — this ticket only reshapes the code so subsequent tickets
can tag real audio threads and consume the allocator from
production binaries.

## Acceptance criteria

- [ ] New crate `patches-alloc-trap` added to the workspace
      with `publish = false`.
- [ ] Crate feature `audio-thread-allocator-trap`, default off.
- [ ] When feature is on: crate exports
      `TrappingAllocator: GlobalAlloc` that forwards to
      `std::alloc::System` but aborts via
      `std::process::abort()` if the calling thread has the
      audio-thread flag set and the process-wide
      `TRAP_ARMED` latch is true.
- [ ] When feature is off: `TrappingAllocator` exists as a
      transparent wrapper that forwards to `System`
      unconditionally. No `std::process::abort` path compiles
      in.
- [ ] Crate exposes a `NoAllocGuard::enter()` /
      `Drop for NoAllocGuard` test utility mirroring the
      existing shape in `alloc_trap.rs`. When the feature is
      off, the guard is a ZST that does nothing.
- [ ] `TRAP_HITS: AtomicUsize` retained for tests that want to
      assert quiet steady-state without relying on abort (useful
      in property tests that warm up then check the counter).
- [ ] Crate compiles clean with and without the feature.
- [ ] `cargo clippy -p patches-alloc-trap --all-features` clean.

## Notes

No `#[global_allocator]` statement in this crate — installation
is the downstream binary's decision. The crate provides the
`TrappingAllocator` *type*; binaries that want the trap declare
`#[global_allocator] static A: TrappingAllocator = …` in
`main.rs` (done in ticket 0593 for `patches-player` and in the
integration-test harness).

The `TRAP_ARMED` latch stays a crate-level `AtomicBool`; the
thread-local flag is exposed via a pub fn
`set_audio_thread_flag(bool)` but the stable API for callers is
`mark_audio_thread()` (ticket 0592).

Depends on nothing. Existing `alloc_trap.rs` stays in place
until ticket 0594 migrates it.
