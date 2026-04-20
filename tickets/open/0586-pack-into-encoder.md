---
id: "0586"
title: pack_into encoder — ParameterMap + ParamLayout → ParamFrame
priority: high
created: 2026-04-19
---

## Summary

Control-thread encoder that writes a `ParameterMap` into a
pre-sized `ParamFrame` using the layout's offsets and buffer
slot indices. Zero allocation after the frame is constructed.
Missing keys fall back to the descriptor default supplied by
the caller.

## Acceptance criteria

- [ ] `pack_into(layout: &ParamLayout, defaults: &ParameterMap,
      overrides: &ParameterMap, frame: &mut ParamFrame)` in
      `patches-ffi-common::param_frame::pack`.
- [ ] For every `ScalarSlot` in `layout.scalars`, write the
      value at `slot.offset` via an unaligned `write_unaligned`
      matching the `ScalarTag`:
      - `Float` → `f32`
      - `Int`   → `i64`
      - `Bool`  → `u8` (0/1)
      - `Enum`  → `u32` variant index
- [ ] For every `BufferSlot` in `layout.buffer_slots`, write
      the id at tail index `slot.slot_index` as `u64`; zero
      means "no buffer bound".
- [ ] Value resolution order per key: `overrides.get(key)` then
      `defaults.get(key)`. If neither supplies a value, panic
      in debug (`debug_assert!`) and zero-fill in release —
      this is a planner bug.
- [ ] `ParameterValue::String` and `ParameterValue::File` are
      rejected at frame-build time: `debug_assert!` panic in
      debug, return `PackError::UnsupportedVariant` in release.
      Spike 5 tightens this to a compile-time split; here we
      assert.
- [ ] Descriptor-hash check: `pack_into` asserts
      `frame.layout_hash() == layout.descriptor_hash`.
- [ ] No allocation inside `pack_into`. Verified by a
      counting-allocator unit test (test-only, the technique
      lands fully in ticket 0590 but a smoke version here is
      fine).
- [ ] Unit tests: every `ScalarTag` round-trips through
      `pack_into` + a raw unaligned read; buffer slots write at
      the right tail indices; override beats default; missing
      key with a default works; unsupported variant errors in
      release.
- [ ] `cargo clippy -p patches-ffi-common` clean; no
      `unwrap`/`expect` in library code.

## Notes

`write_unaligned` because `scalar_size`/offsets are laid out
for compactness, not for `#[repr(C)]` alignment of the whole
struct. The reads in `ParamView` (ticket 0587) will mirror
with `read_unaligned`.

The split `defaults` / `overrides` keeps the caller honest
about which values are planner-provided and which come from
current parameter state; the engine already maintains both
today.
