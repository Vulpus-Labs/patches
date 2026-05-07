---
id: "0830"
title: Drop module-shell boilerplate tests
priority: low
created: 2026-05-07
epic: E138
---

## Summary

Two tests assert against module shell / internals rather than DSP behaviour:

- `patches-modules/src/convolution_reverb/tests.rs:14-25` — walks the
  descriptor and asserts port/param names exist. Pure reflection over a
  static descriptor; cannot regress without the descriptor itself changing.
- `patches-modules/src/poly_filter/tests.rs:225-246` — downcasts to
  `PolyResonantLowpass` and inspects internal `coefs.delta` arrays to
  confirm the static-coefficient fast path. Couples the test to a private
  optimisation; should be expressed as an output-equivalence test instead
  (CV constant ⇒ output bit-identical to a reference run) or dropped.

## Acceptance criteria

- [ ] Convolution-reverb descriptor test removed
- [ ] Poly-filter internal-field test either removed or rewritten as an
      output-level invariant
- [ ] `just inner -p patches-modules` green

## Notes

Descriptor name validation, if needed system-wide, belongs in a
registry-level test, not per-module.
