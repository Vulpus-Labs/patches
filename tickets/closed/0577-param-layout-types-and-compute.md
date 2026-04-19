---
id: "0577"
title: ParamLayout types and compute_layout in patches-ffi-common
priority: high
created: 2026-04-19
---

## Summary

Land the `ParamLayout` types and the pure `compute_layout`
function in `patches-ffi-common`, per ADR 0045 §3 and Spike 1
(E097). No runtime wiring; no dependents yet.

Produces, from a `ModuleDescriptor`:

- `scalar_size: u32` — packed scalar area size, rounded to max
  scalar alignment.
- `scalars: Vec<ScalarSlot>` — `{ key: ParameterKey, offset: u32,
  tag: ScalarTag }`, sorted canonically.
- `buffer_slots: Vec<BufferSlot>` — `{ key, slot_index: u16 }`,
  one per `File` / `FloatBuffer` param.
- `descriptor_hash: u64` — placeholder stub in this ticket
  (literal `0`); real implementation lands in 0578.

`ScalarTag` covers `Float | Int | Bool | Enum` only — matching
the variants that survive Spike 0 on the audio-thread path.
`ParameterKind::String` is gone; `File` / `FloatBuffer` go to
`buffer_slots`.

## Acceptance criteria

- [ ] New `patches-ffi-common::param_layout` module exposes
      `ParamLayout`, `ScalarSlot`, `BufferSlot`, `ScalarTag`, and
      `compute_layout(&ModuleDescriptor) -> ParamLayout`.
- [ ] Canonical ordering: slots sorted by
      `(parameter_name, indexed_position)`.
- [ ] Greedy natural-alignment packing; `scalar_size` rounded up
      to max scalar alignment used.
- [ ] Indexed parameters expand to one slot per element; element
      order reflects index ascending.
- [ ] `buffer_slots[i].slot_index == i as u16`.
- [ ] Unit tests cover: scalar-only descriptor, buffer-only
      descriptor, mixed, indexed params, empty descriptor.
- [ ] No new dependency added to `patches-ffi-common/Cargo.toml`
      without explicit approval.
- [ ] `cargo clippy -p patches-ffi-common` clean.

## Notes

Depends on ADR 0045; companion to 0578 (hash) and 0579 (property
tests). Do not wire this into any dependent crate — Spike 3
consumes it.
