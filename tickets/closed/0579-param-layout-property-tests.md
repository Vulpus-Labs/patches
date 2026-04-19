---
id: "0579"
title: ParamLayout property tests — determinism, alignment, coverage
priority: medium
created: 2026-04-19
---

## Summary

Add property tests for `compute_layout` covering the three
invariants the rest of ADR 0045 relies on:

1. **Determinism.** A generated `ModuleDescriptor` produces the
   same `ParamLayout` and `descriptor_hash` across repeated
   invocations and across a reordered-iteration variant of the
   same descriptor.
2. **Alignment.** For every `ScalarSlot`, `offset %
   align_of(tag) == 0`; `scalar_size` is a multiple of the max
   scalar alignment present; `scalar_size` is the minimum value
   that covers the last slot under greedy packing (no trailing
   waste beyond alignment rounding).
3. **Coverage.** Every parameter in the descriptor appears in
   exactly one of `scalars` / `buffer_slots`, exactly once
   (per index for indexed params).

## Acceptance criteria

- [ ] `proptest` (or equivalent, if already in the workspace) is
      used to generate `ModuleDescriptor` fixtures with mixed
      scalar kinds, buffer kinds, indexed params, and enum
      variants.
- [ ] Three named property tests corresponding to the three
      invariants above.
- [ ] A regression fixture with a known descriptor asserts a
      fixed expected hash value (guards against accidental
      encoding drift).
- [ ] Tests live in `patches-ffi-common/tests/` or
      `src/param_layout/tests.rs` per the crate's existing
      convention.
- [ ] `cargo test -p patches-ffi-common` green.
- [ ] `cargo clippy` clean.

## Notes

Depends on 0577 and 0578. Check `Cargo.toml` for existing
proptest / quickcheck deps before adding anything new; ask
before introducing a new dev-dependency.
