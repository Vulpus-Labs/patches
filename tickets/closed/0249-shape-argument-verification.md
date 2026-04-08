---
id: "0249"
title: Shape argument verification in expander tests
priority: medium
created: 2026-04-02
---

## Summary

`FlatModule::shape` is never asserted on in any test, despite many fixtures
using shape arguments (e.g. `Sum(channels: <size>)`, `Sum(channels: 3)`). We
have no test coverage that shape arguments survive expansion correctly.

## Acceptance criteria

- [ ] In an existing or new expand test, assert that `FlatModule::shape`
      contains the expected entries after expansion of a template that passes
      a literal shape argument (e.g. `Sum(channels: 3)` → shape contains
      `("channels", Scalar::Int(3))`).
- [ ] Assert that shape arguments derived from template parameter substitution
      work correctly (e.g. `Sum(channels: <size>)` with `size: 4` → shape
      contains `("channels", Scalar::Int(4))`).
- [ ] Assert that modules with no shape arguments have an empty `shape` vec.

## Notes

- This can likely be added to the existing `limited_mixer_example_end_to_end`
  or `arity_expansion_basic_three_connections` tests with a few extra assertions.
- Epic: E046
