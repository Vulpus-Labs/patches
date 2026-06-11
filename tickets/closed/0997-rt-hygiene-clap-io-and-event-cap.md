---
id: "0997"
title: "RT hygiene: dlog! I/O on audio thread, host-control event cap, alloc-trap in CI"
priority: high
created: 2026-06-11
---

## Summary

Three real-time-safety defects from the 2026-06 review:

1. **File I/O on the audio thread.** The `dlog!` macro
   (`patches-clap/src/plugin.rs:14-24`) opens/appends/flushes a file per
   call. Two call sites sit inside `plugin_process` (null-process and
   no-processor guards) — a host calling `process` before `activate` does
   file I/O per block.
2. **Unbounded `Vec::push` in host-control scratch.**
   `HostControlScratch::push_event`
   (`patches-engine/src/host_control_scratch.rs`) pushes into a
   `Vec::with_capacity(256)` with no length guard; CLAP `in_events.size`
   is host-controlled, so the 257th automation event in one block
   reallocates on the audio thread.
3. **The alloc trap never fires in CI.** No Justfile tier or CI job
   enables `audio-thread-allocator-trap`, so this defect class
   self-detects nowhere (`patches-clap` doesn't enable the feature at
   all).

## Acceptance criteria

- [ ] No `dlog!` call reachable from `plugin_process` (delete the two
      sites or route through a preallocated lock-free channel drained off
      the audio thread).
- [ ] `push_event` drops (with a drop counter or diagnostic) past
      capacity instead of reallocating; cap documented against
      `MAX_HOST_CONTROLS` / expected event density.
- [ ] One CI tier (smoke is the natural home, see ticket 1002) runs the
      integration suite with `--features audio-thread-allocator-trap`.
- [ ] Secondary: `record_scratch` push in
      `patches-cpal/src/callback.rs:163` guarded against host buffers
      > 8192 frames (truncate + diagnostic rather than realloc).

## Notes

CLAUDE.md audio-engine conventions: no allocations, no blocking, no I/O on
the audio thread. The `params_flush` data race
(`patches-clap/src/extensions.rs:810-853`, TODO references nonexistent
ticket 0825) is adjacent but separately scoped — file it when fixing;
an `AtomicU32` bit-cast f32 for `last_value` is likely sufficient.

## Resolution (2026-06-11)

1. **`dlog!` on the audio thread** — both call sites inside
   `plugin_process` (the null-process and no-processor guards) deleted;
   replaced with audio-thread-safe comments. The `dlog!` macro stays for
   the main-thread lifecycle paths (init/activate/destroy/etc.), which
   are not RT.
2. **Unbounded `push_event`** — `HostControlScratch::push_event` now drops
   past `MAX_HOST_CONTROL_EVENTS` (= 256, the buffer's pre-allocated
   capacity) and bumps a `dropped_events` counter instead of reallocating.
   The buffer is reserved to exactly that capacity, so `push` is
   realloc-free. New test `events_past_cap_dropped_not_reallocated`
   asserts len caps, counter increments, capacity unchanged.
3. **Alloc trap in CI** — delivered via the smoke-tier rework in ticket
   1002 (the natural home, as this ticket anticipated). Smoke now runs
   `cargo test --tests -p patches-integration-tests --features
   audio-thread-allocator-trap`. (The integration crate already exposed
   the feature; nothing armed it in any tier.)
4. **Secondary: `record_scratch`** — push in `patches-cpal` callback now
   guarded by `RECORD_SCRATCH_FRAMES` (= 8192); over-large host buffers
   truncate the recording rather than reallocate. Audio output is
   unaffected.

Out of scope / untouched: the `params_flush` data race
(`extensions.rs`) — left for a separate ticket as the summary noted.

`cargo test -p patches-engine` green; clippy clean on clap/cpal/engine.
