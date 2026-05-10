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

This ticket moves the reserved slots into the scratch region. Wins:
- Halves the storage of 32 backplane slots (16 bytes saved each =
  512 bytes).
- Removes the incidental 1-sample delay on `GLOBAL_TRANSPORT`,
  `GLOBAL_DRIFT`, and `GLOBAL_MIDI` (transport/midi events arrive
  one sample sooner). Consumers that care about exact phase relative
  to host time get a small precision improvement.

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
- [ ] Audit consumers of `GLOBAL_TRANSPORT/MIDI/DRIFT/AUDIO_IN`. If
      any currently rely on the 1-sample-delayed read (no in-tree
      module does as of 0850), document the breaking change.
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
