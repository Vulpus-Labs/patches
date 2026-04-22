---
id: "0630"
title: Hot-reload cycle for vintage bundle through FFI path
priority: medium
created: 2026-04-22
epic: "E109"
---

## Summary

Phase F of Spike 8. Verify that a full hot-reload cycle — plan
adoption, parameter change, port rebind, plan eviction — works
against the bundle-loaded vintage modules and leaves the ArcTable
refcount map converged to zero.

## Acceptance criteria

- [ ] Integration test that loads the vintage bundle, builds a
      plan, runs some frames, swaps in a new plan that (a) changes
      a scalar parameter on a surviving vintage module and (b)
      rebinds at least one port.
- [ ] Assert audio output continuous across the swap (no NaN, no
      panic, no dropout signature).
- [ ] Assert every `FloatBufferId` minted during the run reaches
      zero refcount on final teardown. Use the debug audit path.
- [ ] Assert allocator trap stays clean across the swap.

## Notes

Depends on 0629 landing (shares the alloc-trap harness) and on
the new ABI port-rebind path from Spike 7 Phase D (ticket 0616
or equivalent — check E106).

Not in scope: concurrent multi-plan reloads, reload under live
CLAP host. Both Spike 9 territory.
