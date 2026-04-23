---
id: "0652"
title: Runtime counters for param data plane
priority: medium
created: 2026-04-23
closed: 2026-04-23
epic: "E111"
adrs: ["0045", "0043"]
---

## Summary

Add per-runtime observability counters for the parameter/port data
plane: `ArcTable` capacity, high watermark, growth events,
frame-dispatch rate, pending-release queue depth. Expose via a
snapshot accessor so tests, soaks, and (later) the ADR 0043 tap
surface can sample them.

## Acceptance criteria

- [x] Counters incremented on the relevant hot paths without
      audio-thread allocation (`Relaxed` atomics on a shared
      `Arc<ArcTableCounters>` + one `AtomicU64` for frame dispatch).
- [x] Counters exposed through a snapshot API usable by non-real-time
      consumers: `RuntimeArcTables::snapshot()` and
      `RuntimeAudioHandles::snapshot()` returning
      `RuntimeCountersSnapshot`.
- [x] Documented in the manual under observability
      (`docs/src/engine-internals.md`).

## Out of scope

- **Tap attach integration.** ADR 0043 §6 leaves the attach API
  deferred; once it lands, the observer thread samples `snapshot()`
  on its own cadence. No plumbing required on the counter side.
- **0651 soak wiring.** 0651 depends on 0649/0650 and will consume
  `snapshot()` directly when it lands.

## Implementation

- `patches-ffi-common/src/arc_table/counters.rs` — shared
  `ArcTableCounters` (capacity, high-watermark, growth events,
  releases queued/drained) updated with `Relaxed` atomics, plus
  `ArcTableCountersSnapshot` with a derived `pending_release_depth`.
- `ArcTableControl::mint` observes live-count into the high-water
  mark; `grow` bumps growth events and capacity; `drain_released`
  bumps drained. `ArcTableAudio::release` bumps queued when the
  release reached zero.
- `RuntimeArcTables` / `RuntimeAudioHandles` hold a shared
  `param_frames_dispatched: Arc<AtomicU64>` and expose
  `snapshot() -> RuntimeCountersSnapshot`. Dispatchers increment
  via `RuntimeAudioHandles::note_param_frame_dispatched()`.

## Notes

ADR 0045 §Spike 9. ADR 0043 is the tap infrastructure contract —
reuse, do not fork.
