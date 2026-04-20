---
id: "0587"
title: ParamView reader with perfect-hash name lookup built at prepare
priority: high
created: 2026-04-19
---

## Summary

Audio-thread reader over a `ParamFrame`. Holds a borrowed
reference to the frame bytes plus a precomputed perfect-hash
table mapping `ParameterKey` to slot index. ADR 0045 section 4
specifies O(1) name lookup with no fallback path and no
allocation per call; this ticket delivers that.

## Acceptance criteria

- [ ] `ParamView<'a>` in
      `patches-ffi-common::param_frame::view`, holding:
      - `&'a ParamViewIndex` (the prepare-time perfect hash);
      - `&'a [u8]` scalar area;
      - `&'a [u64]` buffer slots.
- [ ] `ParamViewIndex::from_layout(&ParamLayout) ->
      ParamViewIndex`: builds a minimal perfect hash
      (`ph` crate or hand-rolled CHD — ask before adding a
      dep; hand-rolled FKS or a two-level displacement table
      is acceptable) from the keys in `layout.scalars` and
      `layout.buffer_slots`. Stores per-key `(tag, offset_or_slot)`.
- [ ] Accessors:
      - `fn float(&self, key: impl Into<ParameterKey>) -> f32`
      - `fn int(&self, key: impl Into<ParameterKey>) -> i64`
      - `fn bool(&self, key: impl Into<ParameterKey>) -> bool`
      - `fn enum_variant(&self, key: impl Into<ParameterKey>) -> u32`
      - `fn buffer(&self, key: impl Into<ParameterKey>) -> Option<FloatBufferId>`
        (returns `None` if slot is zero).
- [ ] Each accessor: perfect-hash lookup, tag assert in debug
      (`debug_assert_eq!`), unaligned read. No allocation, no
      branch on lookup path beyond the tag assert.
- [ ] Unknown key: `debug_assert!` panic; release returns the
      type's zero value. Rationale: modules only ask for their
      own declared keys.
- [ ] `ParamView::new(index, frame)` sanity-checks lengths
      (`scalar_area.len() == layout.scalar_size`,
      `buffer_slots.len() == layout.buffer_slots.len()`).
- [ ] Unit tests: round-trip through `pack_into` + `ParamView`
      for every `ScalarTag`; buffer accessor returns `None` on
      zero slot and `Some(id)` on set slot; perfect-hash build
      is deterministic across runs for a given descriptor.
- [ ] Property test: for any descriptor, no two keys collide
      in the built perfect hash.
- [ ] `cargo clippy -p patches-ffi-common` clean.

## Notes

Ask before adding a perfect-hash crate. A hand-rolled FKS
(two-level) with `fxhash` (already transitively pulled by
`pest`? verify) or a deterministic SipHash seed is likely
enough — the key sets are small (tens, rarely >100) and built
once per prepare.

The `ParamViewIndex` is what `prepare` caches alongside the
layout. Lifetime: index outlives every `ParamView` it builds.
