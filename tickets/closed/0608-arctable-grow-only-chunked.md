---
id: "0608"
title: ADR 0045 Spike 6 — grow-only chunked ArcTable with RCU index swap
priority: high
created: 2026-04-21
---

## Summary

ADR 0045 Spike 6. Replace the fixed-capacity `Slots { Arc<[Slot]> }`
refcount storage with a grow-only chunked layout, letting the
per-runtime `ArcTable` adapt to in-session graph growth without
data copies or refcount reconciliation.

Also simplifies the runtime to a single typed `ArcTable<[f32]>`,
retiring the unwired `SongData` placeholder (tracker data uses a
native trait path and does not cross FFI).

## Acceptance criteria

- [x] Chunked slot storage: pinned 64-slot chunks, stable
      addresses, `ChunkIndex` behind `AtomicPtr`.
- [x] RCU-style growth: control thread appends chunks, builds new
      `ChunkIndex`, Release-swaps, retires old index via two-counter
      quiescence barrier (`started` / `completed`).
- [x] `ArcTableControl::grow(additional_slots)` public API.
- [x] `drain_released` also drains retire queue.
- [x] Release ring sized to max capacity so growth never overflows.
- [x] `SongData` placeholder retired; `RuntimeArcTables` holds a
      single `ArcTable<[f32]>`.
- [x] Unit tests: initial-capacity rounding, exhaustion+grow, id
      validity across growth, retire-after-quiescence,
      drop-frees-retired-queue, grow-zero no-op.
- [x] Concurrent soak (`arc_table_grow_under_audio`, `--ignored`):
      4 → 4096 slots under continuous audio-thread retain/release,
      no leaks, no corruption.
- [x] Clippy clean on patches-ffi-common.
- [x] Inner-loop test subset green:
      `cargo test -p patches-core -p patches-modules -p patches-dsp
      -p patches-engine -p patches-ffi-common`.

## Notes

### Why chunked, not copy-and-reconcile

The original ADR sketch had control copy existing slots into a
larger array and swap. That loses refcount deltas: audio-thread
retain/release on the old array during the grace period mutates
refcounts the new copy doesn't reflect. Reconciling requires
tracking every audio-side mutation during the window
(retain-trails, tagged release queues) — new hot-path machinery.

Chunked growth sidesteps this: chunks are pinned, slot addresses
never change, and only the small `ChunkIndex` is RCU-swapped.
Single-buffered slots, double-buffered metadata.

### Sizing

- `CHUNK_SIZE = 64` slots (1 KiB per chunk).
- `MAX_CHUNKS = 1024` → 65 536 slot ceiling per table.
- Release ring sized to the ceiling (512 KiB per table) so grow
  can always queue a release.
- Grow-only within a session; released slots recycle via free
  list. Steady-state capacity tracks peak concurrent live ids.

### Files touched

- [patches-ffi-common/src/arc_table/refcount.rs](../../patches-ffi-common/src/arc_table/refcount.rs):
  full rewrite — chunked storage, `ChunkIndex`, `Quiescence`,
  `SlotsShared`, `RetiredIndex`, `grow` + `drain_retired`.
- [patches-ffi-common/src/arc_table/table.rs](../../patches-ffi-common/src/arc_table/table.rs):
  `ArcTableControl::grow`, release-ring sized to max, drain
  calls `drain_retired`.
- [patches-ffi-common/src/arc_table/runtime.rs](../../patches-ffi-common/src/arc_table/runtime.rs):
  `SongData` retired; single `float_buffers` table; `grow_float_buffers`
  + `float_buffer_capacity` accessors.
- [patches-ffi-common/src/arc_table/mod.rs](../../patches-ffi-common/src/arc_table/mod.rs),
  [patches-ffi-common/src/lib.rs](../../patches-ffi-common/src/lib.rs):
  re-exports updated.
- [patches-core/src/ids.rs](../../patches-core/src/ids.rs): `SongDataId`
  removed; `FloatBufferId` retained with the same API.
- [patches-ffi-common/src/arc_table/soak_tests.rs](../../patches-ffi-common/src/arc_table/soak_tests.rs):
  adjusted for chunk-rounded capacity; added
  `arc_table_grow_under_audio`.
- [adr/0045-ffi-parameter-port-data-plane.md](../../adr/0045-ffi-parameter-port-data-plane.md):
  Spike 6 section rewritten to reflect chunked design; noted
  `SongData` retirement.

### Follow-ups (not this ticket)

- Planner-driven growth calls (ADR 0045 resolved design point 2):
  planner recomputes required capacity on hot-reload and issues
  `grow_float_buffers` ahead of mint pressure. Spike 6 exposes the
  API; wiring lands with planner work.
- Spike 7 (FFI ABI redesign + first external plugin) is the next
  ADR 0045 step.
