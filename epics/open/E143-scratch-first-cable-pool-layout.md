---
id: E143
title: Scratch-first cable-pool layout
status: open
created: 2026-05-10
adr: 0072
---

## Summary

Reorganise the cable-pool index space so backplane and sink slots live
at small, stable cable_idx values in the scratch region, and dyn cycle
producers live at high indices. Drops the `cable_idx - CYCLE_CAPACITY`
arithmetic that 0858 scattered across the engine, harness, and tests,
and fixes the historical accident where disconnected inputs default to
`fused: false` despite being inherently same-tick (constant-zero) reads.

The 0858 push moved the backplane to scratch but left it addressed at
`CYCLE_CAPACITY + N`, on the assumption that sinks had to remain in
cycle. They don't: read sinks are never written, write sinks are never
read, neither needs ping-pong storage. Once sinks join the backplane in
scratch, the index space inverts cleanly:

| range | region | content |
|-------|--------|---------|
| `[0, SINK_SLOTS)` | scratch | sinks (MONO/POLY READ/WRITE) |
| `[SINK_SLOTS, RESERVED_SLOTS)` | scratch | backplane (`AUDIO_OUT_L = 4`, …) |
| `[RESERVED_SLOTS, SCRATCH_CAPACITY)` | scratch | dyn scratch (planner-allocated) |
| `[SCRATCH_CAPACITY, SCRATCH_CAPACITY + CYCLE_CAPACITY)` | cycle | dyn cycle producers |

CablePool dispatch becomes a single cutoff (`idx < SCRATCH_CAPACITY →
scratch[idx]`, else `cycle[idx - SCRATCH_CAPACITY]`). Backplane consts
become small literals (`AUDIO_OUT_L = 4`, `GLOBAL_TRANSPORT = 8`, etc.).
All `slot - CYCLE_CAPACITY` translations in the engine, harness, and
tests disappear.

Concurrently:

- `MonoInput`/`PolyInput`/`StereoInput`/`MidiInput::default()` flips to
  `fused: true`. Disconnected = constant zero = same-tick read by
  definition; the only way to become non-fused is to be wired to a
  delayed-consumer cycle producer, which is the planner's job.
- `cycle_slot_start` (carried as dead diagnostic state in both
  `PlanDecisions` and `ExecutionPlan` post-0858) is dropped or replaced
  with the actual `cycle_hwm` / `scratch_hwm` from `BufferAllocState`.
- `with_cycle_only` (15+ test callers, a foot-gun under fused-true
  defaults) is purged.
- `bench.rs::watermarks` re-derives plan info that `BufferAllocState`
  already tracks; expose it instead.

ADR 0072 grows a phase-5 amendment recording the layout invert and the
fused-true default decision. FFI ABI is changed freely (no external
plugin clients today).

## Tickets

- 0860 — Scratch-first layout + fused-true default. Core invariant
  change. The big one.
- 0861 — Migrate `with_cycle_only` callers; delete the constructor.
- 0862 — Drop `cycle_slot_start`; surface `cycle_hwm`/`scratch_hwm`
  from `BufferAllocState` instead.
- 0863 — Replace `bench.rs::watermarks` re-derive with
  `ExecutionPlan` accessors.

## Sequencing

0860 lands first — it carries the layout invariant. 0861 follows
immediately because removing `with_cycle_only` is part of "no
construction path silently routes backplane reads to an empty
scratch". 0862 and 0863 can land in either order after 0860 and don't
depend on each other.

## Out of scope

- Module-merging fusion (still YAGNI; see speculative notes).
- Parallel execution affinity partitioning (ADR 0072 phase 4 territory).
- Reworking the planner's per-port `cycle/scratch` classification —
  the layout invert is orthogonal to which producers belong in which
  region; phase-1 fusion analysis is unchanged.
