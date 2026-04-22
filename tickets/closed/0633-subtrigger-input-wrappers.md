---
id: "0633"
title: SubTriggerInput / PolySubTriggerInput consumer wrappers
priority: high
created: 2026-04-22
epic: "E103"
adr: "0047"
depends_on: ["0632"]
---

## Summary

Add input wrappers that decode `Trigger` / `PolyTrigger` cables into an
ergonomic `Option<f32>` (mono) or `[Option<f32>; 16]` (poly) per tick,
analogous to the ADR 0030 `TriggerInput` / `GateInput` wrappers but
with no threshold and no prev-state.

## Acceptance criteria

- [ ] `SubTriggerInput` in `patches-core/src/cables.rs` wraps a
      `Trigger` input port. `tick(pool) -> Option<f32>` reads the
      sample; returns `None` if `value < 0.0` else `Some(value)`.
- [ ] `PolySubTriggerInput` same shape over `PolyInput` of kind
      `PolyTrigger`; returns `[Option<f32>; 16]`.
- [ ] `from_ports(inputs, idx)` constructors following the existing
      `TriggerInput::from_ports` pattern.
- [ ] `value()` accessor returning the raw `f32` / `[f32; 16]` for
      cases that need the encoded form (e.g. forwarding).
- [ ] Unit tests: no-event sample returns `None`; event sample at
      `frac = 0.0`, `0.5`, `0.999` returns `Some(frac)`; poly variant
      decodes per-channel independently.

## Notes

No `Copy` / `PartialEq` (matches ADR 0030 precedent for runtime
wrappers, even though this wrapper is stateless — leaves room to add
state later without breaking consumers).
