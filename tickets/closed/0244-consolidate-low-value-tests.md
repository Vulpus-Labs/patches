---
id: "0244"
title: Consolidate low-value tests into parameterised forms
priority: medium
created: 2026-04-01
---

## Summary

Several test files contain groups of near-identical tests that differ only in
a parameter value. Collapsing these into table-driven or parameterised tests
reduces noise, makes it easier to add new cases, and clarifies the property
being tested.

## Acceptance criteria

### patches-dsp

- [ ] `wavetable.rs` — collapse the 4 point-check tests (`mono_zero_is_zero`,
      `mono_quarter_is_one`, `mono_half_is_zero`,
      `mono_three_quarters_is_minus_one`) into a single table-driven test
      iterating over `[(phase, expected)]` pairs.
- [ ] `approximate.rs` — collapse `test_fast_sine_key_points` and
      `test_fast_tanh_key_points` into table-driven forms if they follow the
      same point-check pattern.
- [ ] `noise.rs` — collapse the 3 `*_output_in_range` tests for white, pink,
      brown into a single parameterised test iterating over noise types.

### patches-modules

- [ ] `noise.rs` — same pattern: collapse per-noise-type output range checks.
      Merge `white_output_in_range`, `pink_output_in_range`,
      `red_output_bounded` into one test parameterised over noise type output
      names.
- [ ] `noise.rs` poly — collapse `poly_white_voices_are_independent` and
      similar per-type checks if they share structure.

### patches-core

- [ ] `cables.rs` — collapse the 4 `is_connected_*` tests into a single
      test verifying defaults for all port types.

### patches-dsl

- [ ] `parser_tests.rs` — collapse the 5 positive fixture tests into one
      table-driven test iterating over fixture filenames. Similarly collapse
      the 6 negative fixture tests.
- [ ] `parser_tests.rs` — collapse the 20+ unit-literal tests into a table
      of `(literal, expected_value)` pairs with a single test function.

### Verification

- [ ] All tests pass, zero clippy warnings.
- [ ] No *behavioural* test coverage is lost — the same input/assertion pairs
      must still be exercised.

## Notes

The goal is fewer test *functions* with the same or better coverage and much
clearer intent. Each collapsed test should have a descriptive name like
`wavetable_key_points` or `unit_literal_conversions` and should print the
failing case in its assertion message so failures are still diagnosable.
