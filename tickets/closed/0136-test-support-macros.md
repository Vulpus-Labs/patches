---
id: "0136"
title: Add assert_nearly!, assert_within!, and params! macros to test-support
priority: medium
created: 2026-03-18
---

## Summary

Add three macros to `patches-core/src/test_support/macros.rs` as specified in ADR 0018.
These reduce assertion noise and parameter construction boilerplate in module unit tests.

## Acceptance criteria

- [ ] `assert_nearly!(expected, actual)` asserts
      `(expected - actual).abs() < f32::EPSILON * expected.abs().max(1.0)`.
      Failure message includes expected, actual, and the computed tolerance.
- [ ] `assert_within!(expected, actual, delta)` asserts
      `(expected - actual).abs() < delta`.
      Failure message includes expected, actual, and delta.
- [ ] `params![key => value, ...]` expands to `&[(&str, ParameterValue), ...]` where:
      - An `f32` literal maps to `ParameterValue::Float`.
      - A `bool` literal maps to `ParameterValue::Bool`.
      - A `&str` literal maps to `ParameterValue::Enum`.
      - An `i64`-suffixed integer maps to `ParameterValue::Int`.
- [ ] All three macros are re-exported from `patches_core::test_support` under the
      `test-support` feature gate.
- [ ] Unit tests cover: `assert_nearly` passes for values near unity; `assert_nearly`
      passes for values near 440 (verifying the scaling); `assert_within` passes and
      fails at the boundary; `params!` produces the correct `ParameterValue` variants.
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no warnings.

## Notes

`assert_nearly` argument order follows `assert_eq!` convention: expected first, actual
second. This ensures failure messages read "expected X, got Y".

`f32::EPSILON` is approximately `1.19e-7`. For a value of 440 Hz, the scaled tolerance
is `440 * f32::EPSILON ≈ 5.2e-5` — still tight enough to catch real errors but loose
enough to pass for numerically correct results.

The `params!` macro does not need to handle mixed-index parameters (e.g. `"level/1"`).
`ParameterValue` does not carry an index; the index is part of `ParameterKey` which is
handled by the map. Plain string keys with implicit index 0 are sufficient for the common
test case.
