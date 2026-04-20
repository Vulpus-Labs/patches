---
id: "0585"
title: ParamFrame buffer type with scalar area + tail u64 slot table
priority: high
created: 2026-04-19
---

## Summary

Define `ParamFrame`, the owned byte buffer that ADR 0045
section 3 describes: a fixed-size scalar area followed by a
tail array of `u64` buffer slot ids, sized from a module
instance's `ParamLayout`. This ticket only defines the type and
its constructor/reset surface — no encoder, no reader, no SPSC
wiring.

## Acceptance criteria

- [ ] `ParamFrame` lives in `patches-ffi-common::param_frame`
      as a new sibling module to `param_layout` and `arc_table`.
- [ ] Owns `bytes: Vec<u8>` whose length is exactly
      `layout.scalar_size as usize + layout.buffer_slots.len() *
      size_of::<u64>()`. Capacity equals length at
      construction; length and capacity never change after
      construction.
- [ ] `ParamFrame::with_layout(&ParamLayout) -> Self` allocates
      the vec, zero-fills it, and records `scalar_size` and
      `buffer_slot_count` as `u32` fields for fast split.
- [ ] `scalar_area(&self) -> &[u8]` and
      `scalar_area_mut(&mut self) -> &mut [u8]` return the
      first `scalar_size` bytes.
- [ ] `buffer_slots(&self) -> &[u64]` and
      `buffer_slots_mut(&mut self) -> &mut [u64]` return the
      tail `u64` slice. Use `bytemuck::cast_slice(_mut)` (add
      `bytemuck` only if not already a workspace dep — ask
      first; otherwise hand-roll with `align_to` and assert
      alignment at construction).
- [ ] `reset(&mut self)`: zero the bytes in place. No
      reallocation.
- [ ] `layout_hash(&self) -> u64`: stores the
      `descriptor_hash` from the layout at construction for
      later sanity checks.
- [ ] Unit tests: round-trip write/read of scalar bytes and
      buffer slots, reset clears to zero, `with_layout` on a
      zero-param layout produces a zero-length frame.
- [ ] `cargo clippy -p patches-ffi-common` clean.

## Notes

No `unsafe` beyond what `bytemuck` (or the hand-rolled cast)
requires. If going hand-rolled: assert 8-byte alignment of the
tail by choosing the tail start to be `align_up(scalar_size,
8)` — but that diverges from the ADR wire format. Simpler: the
scalar area already ends on a multiple of its max alignment
(per spike 1 property tests, `scalar_size` is rounded up); if
that max is `<8` we over-align by padding `scalar_size` to a
multiple of 8 inside `ParamFrame` but **do not** mutate the
`ParamLayout` itself — the padding is internal to the frame's
`Vec<u8>` shape. Document this explicitly.

Belongs in `patches-ffi-common` because both host and plugin
sides need it once the FFI spike lands. No `patches-ffi` dep
yet.
