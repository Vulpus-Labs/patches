---
id: "0741"
title: Tap identity (tap_type, name) and source-order slot mapping
priority: high
created: 2026-04-27
---

## Summary

Drop the global alphabetical sort and the name-uniqueness rule from
ADR 0054 §3. Tap identity is `(tap_type, name)`; slot ordering is the
source location of the tap target. Two taps of different type with the
same name (e.g. `~trigger_led(kick)` and `~meter(kick)`) coexist.

## Acceptance criteria

- [ ] Desugarer/planner walks tap targets in source order; assigns slot
      offsets sequentially (using widths from 0740).
- [ ] Manifest `TapDescriptor` keys observer state by
      `(tap_type, name)`.
- [ ] Parser drops the "tap names must be unique" diagnostic; instead
      diagnoses `(tap_type, name)` collisions.
- [ ] Renaming a single tap shifts only that tap's name in the
      manifest's name index, not the slot table.
- [ ] LSP hover / go-to-definition for tap targets unchanged from the
      user's perspective.

## Notes

ADR 0059 §6. Source order = byte offset of the `~taptype(name, ...)`
token in the post-include, post-expansion source (use the existing
provenance tag's primary span).

Watch for tests that asserted alphabetical slot order in
`patches-observation` or planner — update assertions to match source
order.
