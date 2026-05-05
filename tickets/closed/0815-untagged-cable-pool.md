---
id: "0815"
title: Drop CableValue enum tag; fixed [f32; 16] cable slots
priority: high
created: 2026-05-04
epic: E136
---

## Summary

Replace the `CableValue` enum (`Mono(f32) | Poly([f32; 16])`) with a
fixed-width `[f32; 16]` per cable slot (ADR 0068 §1). Cable kind is
already determined statically at bind time; the runtime tag duplicates
information the planner owns. Removing it eliminates a `match` per
read, opens the path to a contiguous `memcpy` across the host-control
backplane region (ticket 0816), and simplifies the FFI raw-parts API.

The slot stays 16-wide so a slot used for Mono in one plan can be
repurposed as Poly in a later plan without reallocating the cable
pool.

## Acceptance criteria

- [x] `CableValue` is `[f32; 16]` (or a transparent newtype thereof);
      no enum variants.
- [x] `CablePool::read_mono` / `read_stereo` / `read_poly` access the
      slot directly; no `match`, no unreachable arms.
- [x] `CablePool::write_*` symmetric: writers know their kind and
      write the relevant prefix. Bytes beyond the prefix are
      explicitly documented as unspecified (not zeroed).
- [x] FFI raw-parts API updates to `(*mut [f32; 16], len, wi)`. Test
      plugins (`test-plugins/`) rebuilt against the new ABI.
- [x] `midi_io` sentinel-via-Mono mechanism still works (sentinel
      stored in `slot[0]`); existing `midi_io` tests pass unchanged.
- [x] No behavioural change for any existing module: full `just push`
      passes, including integration tests and the determinism /
      hash-stability suite.
- [x] Engine micro-benchmarks (where they exist) show no regression
      and ideally a small win on Poly-heavy patches.

## Notes

- The change touches every cable consumer but each diff is
  mechanical. Treat as a single PR; reviewer's job is to confirm
  every read/write site picked up the right prefix.
- Don't introduce `unsafe` to skip bounds checks on the slot index;
  the compiler elides them for `[f32; 16]` indexed by a constant
  prefix.
- Document in the cable-pool module rustdoc that bytes beyond the
  written prefix are unspecified — modules must not read them.
- ADR 0057 §4 ("sub-block automation parked") is unaffected by this
  ticket; it gets amended by 0816.
