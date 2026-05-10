---
id: "0858"
title: Promote reserved infrastructure slots from cycle to scratch region
priority: low
created: 2026-05-10
epic: E141
adr: 0072
depends-on: "0850"
---

## Summary

After ticket 0850 the cable pool is split into a cycle region
(`[CableValue; 2]` per slot, `[0, CYCLE_CAPACITY)`) and a scratch
region (single `CableValue` per slot, `[CYCLE_CAPACITY, ...)`).
Reserved infrastructure slots (`AUDIO_OUT_L/R`, `AUDIO_IN_L/R`,
`GLOBAL_TRANSPORT`, `GLOBAL_DRIFT`, `GLOBAL_MIDI`, `TAP_BASE..+4`,
`HOST_CONTROL_BASE..+4`) were left in the cycle region to preserve
bit-identical audio for backplane consumers, even though their
write/read semantics are inherently same-tick (engine writes
`pool[X][wi]`, modules read with `fused: false` getting `[1-wi]` =
prior tick — a 1-sample delay that no consumer of these slots actually
relies on by design).

This ticket moves the reserved slots into the scratch region. The
motivation is **logical consistency, not perf or capacity**.

Backplane slots are inherently single-slot-shaped: the engine writes
each one once per tick before any module runs, and every consumer
reads same-tick. There is no cyclic alternation, no feedback edge,
no reason for ping-pong storage. Leaving them in the cycle region
contradicts the rule the rest of the cable pool follows ("cycle =
delayed-consumer producers"), and that contradiction is a trap for
future maintenance — particularly LLM-authored changes that reason
from the layout invariant. A consistent rule prevents erroneous
decisions like "this slot must be cyclic because it's in cycle
range" or "I can write `[wi]` here because all cycle writes use the
ping-pong pattern".

Secondary effects (not motivations):

- Storage shrinks by ~2 KB (32 slots × 64 B each). Rounding noise.
- Cycle capacity for dyn producers rises 96 → 128. Not load-bearing
  — could be reclaimed by resizing or rebalancing the regions.
- `GLOBAL_TRANSPORT/DRIFT/MIDI` arrive one sample sooner. Incidental;
  no in-tree consumer depended on the prior delay.

No per-tick perf benefit (same scratch dispatch cost). 0859's
benchmarks should not target 0858.

Audio goldens that depended on the prior delay (none expected, but
worth auditing) get regenerated.

## Acceptance criteria

- [ ] Move the reserved-slot constants from `[0, RESERVED_SLOTS)` (cycle)
      to a fixed range inside the scratch region. Either:
      - Pin reserved scratch slots at `[CYCLE_CAPACITY, CYCLE_CAPACITY + RESERVED_SLOTS)`,
        keeping the cycle region purely for dynamic feedback producers; OR
      - Flip the cutoff direction (scratch low, cycle high) and put
        reserved at `[0, RESERVED_SLOTS)` in scratch with cycle above
        a per-engine cutoff.
- [ ] Engine `init_cycle_pool` no longer pre-zeros poly slots in the
      reserved range; `init_scratch_pool` pre-zeros the appropriate
      reserved scratch indices.
- [ ] Engine `tick()` reserved-slot writes go through `scratch_pool`.
      Drop the `[wi]` indexing on those sites.
- [ ] Flip every backplane reader's `fused` flag to `true`. Scratch
      reads in `CablePool::read_raw` debug-assert `fused`; today
      every reader uses `MonoInput::scalar` / `PolyInput::scalar`
      (default `fused: false`), so without this step the assert
      fires in every test and release builds silently get same-tick
      reads via the wrong code path.
      - Make `MonoInput::backplane(idx)` / `PolyInput::backplane(idx)`
        / `MidiInput::backplane(idx)` the canonical fused-by-default
        constructor for backplane reads (existing `PolyInput::backplane`
        currently delegates to `scalar` and inherits `fused: false` —
        change it).
      - Update call sites: `audio_in` (AUDIO_IN_L/R), `oscillator` +
        `poly_osc` (GLOBAL_DRIFT), `host_transport` +
        `master_sequencer` (GLOBAL_TRANSPORT), all MIDI consumers
        (~7 modules using `MidiInput::backplane(GLOBAL_MIDI)`),
        `host_control` (HOST_CONTROL_BASE — already uses `backplane`,
        picks up the fix automatically).
- [ ] Audit consumers for any module that *did* rely on the
      1-sample-delayed read of `GLOBAL_TRANSPORT/MIDI/DRIFT/AUDIO_IN`
      (no in-tree module does as of 0850). Document any breaking
      change.
- [ ] `pool_slot(idx)` and `tap_backplane()` continue to expose the
      reserved range correctly under the new layout.
- [ ] All audio integrity + feedback regression tests still pass.
      Audio golden fixtures regenerate if needed; document deltas
      (expected to be timing-only on transport-driven patches).
- [ ] `just push` clean.

## Notes

Sized at the time of 0850, `CYCLE_CAPACITY = 128` allows
`RESERVED_SLOTS (32) + 96` dynamic cycle headroom. Promoting reserved
to scratch frees those 32 slots for dynamic cycle, raising the
per-plan cycle ceiling without growing the constant.

Direction choice (cycle high vs cycle low) interacts with the FFI
ABI: ABI v10 already takes split pointers, so a cutoff direction flip
is internal and does not require ABI v11. Plugin port encoding
(`cable_idx` is opaque to the plugin SDK) is unchanged.

Source-compat caveat for FFI plugins: the *wire* format is unchanged,
but the literal values of `AUDIO_OUT_L`, `GLOBAL_TRANSPORT`, etc. shift
by `CYCLE_CAPACITY`. Any plugin that imports these constants from
`patches-core` (or `patches-ffi-common`) and bakes the literal must be
rebuilt. Decide whether to bump ABI minor as a forcing function or rely
on cargo's normal rebuild — leaning towards no bump since plugins
normally pin patches-core via path/git, but call out in release notes.
