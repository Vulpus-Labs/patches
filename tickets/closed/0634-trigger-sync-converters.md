---
id: "0634"
title: TriggerToSync / SyncToTrigger converter modules
priority: medium
created: 2026-04-22
epic: "E103"
adr: "0047"
depends_on: ["0632", "0633"]
---

## Summary

Add two stateless converter modules in `patches-modules` that bridge
the ADR 0030 threshold-trigger world (`Mono`/`Poly` 0/1 pulses) and
the ADR 0047 sub-sample trigger world (`Trigger`/`PolyTrigger` with
`-1` / `frac` encoding). Explicit conversion makes the semantic
boundary visible in patches.

## Acceptance criteria

- [ ] `TriggerToSync` — `Mono` in → `Trigger` out. Uses
      `TriggerInput` internally (ADR 0030 rising-edge detection at
      threshold 0.5). Emits `0.0` on rising-edge samples, `-1.0`
      otherwise. Document the precision loss: events snap to sample
      boundaries.
- [ ] `SyncToTrigger` — `Trigger` in → `Mono` out. Uses
      `SubTriggerInput`. Emits `1.0` on event samples, `0.0`
      otherwise. Document the fractional-position loss.
- [ ] `PolyTriggerToSync` / `PolySyncToTrigger` poly variants.
- [ ] Each variant has a unit test confirming round-trip through the
      pair loses sub-sample precision (frac → snapped) but preserves
      event count and ordering.
- [ ] Module doc comments follow the manual standard (CLAUDE.md).

## Notes

These modules are small and stateless. Live under
`patches-modules/src/`; no shared core needed.
