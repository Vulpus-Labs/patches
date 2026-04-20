---
id: "0588"
title: Three-SPSC frame shuttle with free-list recycling and coalescing
priority: high
created: 2026-04-19
---

## Summary

Per-module-instance transport for `ParamFrame` values, as ADR
0045 section 3 describes: a pre-sized free-list, a dispatch
queue control→audio, a cleanup queue audio→cleanup-thread, and
a pre-queue coalescing slot table keyed by `(module_idx,
ParameterKey)`. Uses `rtrb` SPSCs (already a workspace dep).

## Acceptance criteria

- [ ] New module
      `patches-ffi-common::param_frame::shuttle`.
- [ ] `ParamFrameShuttle` per instance with three `rtrb`
      SPSCs:
      - `dispatch: Producer<ParamFrame>` on control,
        `Consumer<ParamFrame>` on audio;
      - `cleanup: Producer<ParamFrame>` on audio,
        `Consumer<ParamFrame>` on a cleanup thread (or the
        engine's existing cleanup channel — pick whichever
        matches `patches-engine`'s current design, reuse not
        duplicate);
      - `free: Producer<ParamFrame>` from cleanup,
        `Consumer<ParamFrame>` on control.
- [ ] Constructor `with_capacity(layout: &ParamLayout, depth:
      usize)` pre-fills the free-list with `depth` frames, each
      built via `ParamFrame::with_layout`. `depth` is caller-
      supplied for this spike; planner-derived sizing is a
      later concern.
- [ ] Coalescing slot table: `Vec<Option<ParamFrame>>` indexed
      by a `(module_idx, ParameterKey)`-derived dense index,
      living on the control thread in front of `dispatch`. On
      update:
      1. Pop a frame from `free` if the slot is empty;
         otherwise reuse the slot's existing frame (last-wins).
      2. `pack_into` into the frame.
      3. Flush policy: push to `dispatch` at the end of the
         control-thread update tick (caller's responsibility —
         expose `flush(&mut self)`), not per-key.
- [ ] Audio consumer: `pop_dispatch()` returns the next frame
      or `None`; after use, push to `cleanup`.
- [ ] Cleanup consumer: pop from `cleanup`, run any
      per-frame cleanup (Arc release slot walk — stubbed here,
      fully wired in spike 7), `frame.reset()`, push to `free`.
- [ ] Back-pressure: `free` empty on the control thread is a
      drop-pending update (log + counter); never allocate to
      satisfy demand.
- [ ] Unit tests (single-threaded with manual SPSC drives):
      - Round-trip a frame through dispatch → cleanup → free.
      - Coalescing: two updates to the same key between flushes
        produce one dispatched frame carrying the later value.
      - Free-list exhaustion returns a "drop" signal, no alloc.
- [ ] `cargo clippy -p patches-ffi-common` clean.

## Notes

Investigate whether the coalescing slot table is better placed
in `patches-engine` (where `module_idx` is meaningful) with
only the per-instance SPSC triplet living in `patches-ffi-
common`. If so, split this ticket's scope at implementation
time and note the split in the ticket body before closing.

Cleanup-thread wiring: `patches-engine` already has a
parameter-map cleanup path (destructive take from in-process
path). Reuse that thread where practical; don't spawn a new
one.

## Closed notes

Landed in patches-ffi-common/src/param_frame/. Tests green; clippy clean.
