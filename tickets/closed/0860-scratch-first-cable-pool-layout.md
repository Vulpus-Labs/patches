---
id: "0860"
title: Invert cable-pool layout — scratch low, cycle high; fused-true default
priority: high
created: 2026-05-10
epic: E143
adr: 0072
---

## Summary

Reorganise the cable-pool index space so the scratch region occupies
low cable_idx and the cycle region high. Sinks and backplane move to
fixed positions at the bottom of scratch. `MonoInput`, `PolyInput`,
`StereoInput`, and `MidiInput` `Default` impls flip `fused: true`,
matching the fact that disconnected reads are inherently same-tick
constant zero.

The result: backplane consts return to small literals (`AUDIO_OUT_L =
4`, `GLOBAL_TRANSPORT = 8`, …); the engine writes
`scratch_pool[AUDIO_OUT_L]` directly with no arithmetic; the harness's
four dual-region accessors collapse to one each; `CablePool` dispatch
is a single cutoff (`idx < SCRATCH_CAPACITY`).

ADR 0072 grows a phase-5 amendment recording the invert.

## Acceptance criteria

- [x] New layout in `patches-core/src/cables/mod.rs`:
  - `MONO_READ_SINK = 0`, `POLY_READ_SINK = 1`, `MONO_WRITE_SINK = 2`,
    `POLY_WRITE_SINK = 3`. (Storage: scratch.)
  - `SINK_SLOTS = 4`.
  - `AUDIO_OUT_L = 4`, `AUDIO_OUT_R = 5`, `AUDIO_IN_L = 6`, `AUDIO_IN_R = 7`,
    `GLOBAL_TRANSPORT = 8`, `GLOBAL_DRIFT = 9`, `GLOBAL_MIDI = 10`,
    `TAP_BASE = 11`, `HOST_CONTROL_BASE = 15`. (Storage: scratch.)
  - `RESERVED_SLOTS = 32` (bottom of scratch — sinks + backplane).
  - `SCRATCH_CAPACITY` — fixed pool-wide constant; pick a value that
    covers dyn-scratch demand for the largest in-tree example with
    headroom (current bench shows ≤ ~1200 dyn scratch slots used;
    propose `SCRATCH_CAPACITY = 2048`).
  - `CYCLE_CAPACITY = 128` unchanged. Cycle cable_idx range:
    `[SCRATCH_CAPACITY, SCRATCH_CAPACITY + CYCLE_CAPACITY)`.
- [x] `CablePool::new` dispatch: `cable_idx < SCRATCH_CAPACITY →
      scratch[cable_idx]` else `cycle[cable_idx - SCRATCH_CAPACITY][...]`.
      Document the single cutoff in the doc comment; delete the
      "two virtual number spaces" framing in favour of "scratch low,
      cycle high".
- [x] `Default` for `MonoInput`, `PolyInput`, `StereoInput`,
      `MidiInput` sets `fused: true`. `connected: false`. Reads from
      `cable_idx = *_READ_SINK` route to scratch[0..2], constant zero,
      same-tick.
- [x] `MonoInput::backplane` / `PolyInput::backplane` /
      `MidiInput::backplane` stay (named constructor for clarity at
      call sites), still set `fused: true`. Functionally redundant with
      `Default + cable_idx = X + connected: true`, but explicit at the
      call site is worth keeping.
- [x] `read_raw` debug-assert: `fused → cable_idx < SCRATCH_CAPACITY`.
      Today's assert (`fused → cable_idx >= CYCLE_CAPACITY`) is the same
      idea with the cutoff renamed.
- [x] `init_scratch_pool` allocates `[CableValue; SCRATCH_CAPACITY]`
      (or `min(SCRATCH_CAPACITY, buffer_capacity)` if callers still
      pass `buffer_capacity`; default to `SCRATCH_CAPACITY` and
      consider dropping the parameter).
- [x] `init_cycle_pool` allocates `[[CableValue; 2]; CYCLE_CAPACITY]`,
      poly-init unchanged (cosmetic per ADR 0068).
- [x] Planner allocator:
  - Cycle hwm starts at `0` (no sinks in cycle now), capped at
    `CYCLE_CAPACITY`. Cycle cable_idx emitted as `SCRATCH_CAPACITY +
    hwm`.
  - Scratch hwm starts at `RESERVED_SLOTS` (skip the backplane range),
    capped at `SCRATCH_CAPACITY`.
- [x] Engine `tick()`, `write_input`, halt path, `snapshot_tap_lanes`,
      `tap_backplane`: all `scratch_pool[X - CYCLE_CAPACITY]` sites
      become `scratch_pool[X]`. Cycle accesses (none today on backplane;
      only via `CablePool` from inside modules) keep using the dispatch.
- [x] `patches-core::test_support::harness::ModuleHarness`:
  - `pool_value` / `set_pool_value` / `pool_slot` / `set_pool_slot`
    collapse to single-dispatch through `CablePool`'s region rule.
  - User-cable layout updates: with sinks in scratch, the harness's
    user inputs/outputs sit at known cable_idx positions. Pick a clear
    scheme (e.g. dyn-scratch base for user cables) and document.
- [x] `midi_io`, `host_control` tests: scratch base for backplane is
      `0` (no `- CYCLE_CAPACITY`).
- [x] FFI: change the wire format and bump ABI version to v11. No
      external clients; in-tree consumers (gain, conv-reverb test
      plugins, the host loader, patches-ffi tests) recompile against
      the new layout.
- [x] `patches-vintage` modules read `GLOBAL_DRIFT` via the same
      `MonoInput::backplane` path — verify after the const shift.
- [x] Audio integrity + feedback regression tests pass. Audio
      goldens may regenerate (transport timing shifts by one sample
      on patches that read `GLOBAL_TRANSPORT/MIDI/DRIFT` — same
      condition as 0858).
- [x] `just push` clean.

## Notes

ADR 0072 amendment captures three load-bearing decisions:

1. **Scratch is low, cycle is high.** Inverts the 0850/0858 framing.
   Justification: backplane (the most-trafficked reserved region) is
   naturally single-slot; making it the bottom of the index space
   gives small const literals and zero arithmetic at engine
   write-sites.
2. **Sinks move to scratch.** Read sinks never written; write sinks
   never read. Neither needs ping-pong storage. Falling out of (1):
   sinks at scratch `[0, 4)`, backplane at scratch `[4, 32)`.
3. **`fused: true` is the default for disconnected inputs.**
   Disconnected ports always read constant zero same-tick; the only
   transition out of `fused: true` is being wired to a delayed-consumer
   producer (a planner decision). Today's `fused: false` default is a
   historical accident from when fusion was opt-in.

FFI ABI v11: the wire format change is the cable_idx number-space
flip. Plugin SDKs that bake any of the backplane consts need rebuild;
patches-ffi-common is the single source for the constants so the
recompile catches everything.

Sequencing within the ticket:

1. Add `SCRATCH_CAPACITY` const; rewrite `cables/mod.rs` layout
   comments and consts.
2. Flip `CablePool` dispatch.
3. Flip `Default::fused`.
4. Update planner allocator boundaries.
5. Sweep engine + harness + tests for `- CYCLE_CAPACITY` arithmetic.
6. Audit `read_raw` debug-asserts.
7. ABI bump; rebuild FFI consumers.
8. Regenerate audio goldens if needed.

Anticipated audio-golden churn: same set as 0858 (transport-driven
patches advance one sample sooner). Should be re-auditioned, not
silently regenerated.
