---
id: "0632"
title: Add Trigger / PolyTrigger cable kinds to core
priority: high
created: 2026-04-22
epic: "E103"
adr: "0047"
---

## Summary

Introduce `CableKind::Trigger` and `CableKind::PolyTrigger` as peers of
`Mono` and `Poly`. Buffer layout is identical to their mono/poly
counterparts (`f32` and `[f32; 16]`); the distinction is a type tag
enforced at graph connection time. Encoding: `0.0` = no event on this
sample, `(0.0, 1.0]` = fractional sub-sample position of an event.
Silent value matches the pool's default-zero state, so no per-tick
clearing is needed.

## Acceptance criteria

- [ ] `CableKind` gains `Trigger` and `PolyTrigger` variants in
      `patches-core/src/cables.rs`.
- [ ] Buffer pool allocates `Trigger` like `Mono` (single `f32`) and
      `PolyTrigger` like `Poly` (`[f32; 16]`).
- [ ] No special fill needed: pool's existing `CableValue::Mono(0.0)` /
      `CableValue::Poly([0.0; 16])` init doubles as the "no event"
      sentinel. Producers write every sample (`0.0` silent, `frac`
      on events) — same discipline as normal audio cables.
- [ ] Graph validation rejects connections across incompatible kinds;
      `Trigger ↔ Trigger` and `PolyTrigger ↔ PolyTrigger` only. Error
      flows through existing `GraphError::CableKindMismatch`.
- [ ] `param_layout::port_kind_tag` gains two new tag values.
- [ ] `test_support::harness` accepts both new kinds in `input_kinds` /
      `output_kinds` and constructs appropriate input/output wrappers.
- [ ] `PortDescriptor` builder/DSL can declare trigger ports
      (e.g. `trigger_in("sync")`, `trigger_out("reset_out")`,
      `poly_trigger_in`, `poly_trigger_out`).
- [ ] New core unit tests covering: kind mismatch rejection, default
      `0.0` silence read, poly layout interaction.

## Notes

No implicit coercion to/from `Mono` / `Poly`; converters come in 0634.
The `0.0` silent value piggy-backs on the pool's default-zero init so
producers only write on event samples; stale values between events are
overwritten each tick by the write slot being in the normal ping-pong
write path (modules write `0.0` on non-event samples).
