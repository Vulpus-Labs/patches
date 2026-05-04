---
id: "0814"
title: Fold tap backplane into reserved cable-pool region
priority: high
created: 2026-05-04
---

## Summary

Today the tap "backplane" is a parallel `[f32; MAX_TAPS]` slice on
`PatchProcessor`, threaded into `CablePool` via the
`backplane: Option<&mut [f32]>` field and a dedicated
`write_backplane` API. This is an artefact: the cable pool already
hosts every other backplane (`AUDIO_OUT_L`, `GLOBAL_MIDI`,
`GLOBAL_TRANSPORT`, …) as reserved slots, and the tap data could just
live there too.

Folding the tap backplane back into the cable pool simplifies three
things at once:

- one source of truth for shared audio-thread state (the cable pool),
- `CablePool` loses its `backplane` field and `write_backplane`
  method (one fewer cross-cutting API),
- FFI plugins automatically gain the ability to be tap-shaped (the
  cable-pool pointer already crosses the FFI boundary; the parallel
  slice does not).

This is preparation for ticket 0809 (host-control module). 0809 also
needs a backplane region; landing a single, consistent reserved-region
layout first avoids two parallel kludges.

## Layout

Reserved region is bumped to **32 slots** (power of two). New layout:

| Slot     | Name                | Kind | Lanes |
|----------|---------------------|------|-------|
| 0–3      | read/write sinks    | mix  | —     |
| 4–7      | `AUDIO_OUT_{L,R}`, `AUDIO_IN_{L,R}` | Mono | 1 each |
| 8        | `GLOBAL_TRANSPORT`  | Poly | 16    |
| 9        | `GLOBAL_DRIFT`      | Mono | 1     |
| 10       | `GLOBAL_MIDI`       | Poly | 16    |
| 11–14    | `TAP_BASE..+4`      | Poly × 4 | 64 (= existing `MAX_TAPS`) |
| 15–16    | `HOST_CONTROL_BASE..+2` | Poly × 2 | 32 (`MAX_HOST_CONTROLS`, ticket 0809) |
| 17–31    | spare / future      | —    | —     |

`RESERVED_SLOTS = 32`. Dynamic cables start at 32.

Slot indexing: lane `n % 16` of slot `BASE + n / 16` for both tap and
host-control regions. Stereo tap channels claim two consecutive lanes;
the desugarer already handles this (ADR 0059 §5).

## Acceptance criteria

- [ ] `patches-core::cables::mod`: introduce `TAP_BASE`, `TAP_SLOTS`,
      `HOST_CONTROL_BASE`, `HOST_CONTROL_SLOTS`, `MAX_HOST_CONTROLS`.
      `RESERVED_SLOTS = 32`. `MAX_TAPS = TAP_SLOTS * 16` (= 64,
      preserving the existing public value).
- [ ] `patches-engine::kernel::init_buffer_pool`: zero-init the four
      tap poly slots and the two host-control poly slots
      (`Poly([0.0; 16])`).
- [ ] `Tap::process`: build a `[f32; 64]` accumulator from the
      module's channel reads, then write four `Poly` cables to
      `TAP_BASE..TAP_BASE+4` at end of tick. Stereo channels still
      land L at `slot_offset`, R at `slot_offset + 1` within the
      lane space.
- [ ] `PatchProcessor`: drop the `tap_backplane: TapFrame` field and
      the `with_backplane` call site. After `tick()`, snapshot
      `pool[TAP_BASE + i][read_idx]` (i = 0..4) into the
      `TapBlockFrame` for the observer ring.
- [ ] `CablePool`: remove `backplane: Option<&mut [f32]>` field,
      `with_backplane` constructor, and `write_backplane` method.
      `tap_backplane()` accessor on `PatchProcessor` returns a
      `[f32; MAX_TAPS]` reconstructed from the four poly slots
      (single read of the read-side ping-pong).
- [ ] `ModuleHarness`: drop `enable_backplane`, `backplane()`. Add
      `tap_backplane_lane(slot) -> f32` that reads from the cable
      pool for tap-module tests.
- [ ] All existing tap-module tests pass against the new path.
- [ ] `just inner -p patches-core -p patches-modules -p patches-engine`
      passes; `just inner -p patches-clap` passes (FFI boundary
      unchanged).

## Out of scope

- Host-control module (ticket 0809; this ticket only reserves the
  region).
- Manifest emission for host controls (ticket 0810).
- Removing `Option<&mut [f32]>` from anywhere it survives only as
  test scaffolding the harness still uses post-refactor.

## Notes

- ADR 0053 §4 calls the tap region a "backplane"; ADR 0057 §4 calls
  the host-control region a backplane too. Both terms survive — they
  describe a *role* (audio-thread shared backplane) rather than a
  storage mechanism. Storage is now uniformly the cable pool.
- ADR 0056 / `TapBlockFrame` shape is unchanged; the engine still
  accumulates per-sample frames into a block frame and pushes to the
  observer ring. The only change is *where* the per-sample values
  come from.
