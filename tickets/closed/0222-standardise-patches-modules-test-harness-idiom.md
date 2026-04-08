---
id: "0222"
title: Standardise patches-modules test harness idiom
priority: low
created: 2026-03-30
---

## Summary

Minor inconsistencies in harness setup and assertion style have accumulated
across the six test modules. This ticket standardises them so that all files
follow the same idiom, reducing cognitive overhead when reading or extending any
module's tests.

## Acceptance criteria

- [ ] **`AudioEnvironment` construction** — every test file constructs the
  environment inside its `make_*` factory helper (or a shared `env()` helper if
  the file tests more than one module type, as in `noise.rs`). No test function
  should inline `AudioEnvironment { ... }` itself.
- [ ] **`assert_within!` tolerances are documented** — every call to
  `assert_within!` has a short inline comment explaining the chosen epsilon (e.g.
  `// lookup table has ~1e-4 max error`, `// PolyBLEP is order-1; up to ~1% error at transitions`).
  Comments need only be added where the epsilon is not obvious from context;
  absolute comparisons to 0.0 need no comment.
- [ ] **`PolyNoise` smoothness test** — add a poly equivalent of
  `noise::tests::brown_smoother_than_white_and_red_smoother_than_brown` that
  verifies the same MAD ordering holds for each of the 16 voices individually.
  (The poly PRNG path is distinct from the mono path; this is not redundant with
  the existing test.)
- [ ] All existing passing tests remain green.
- [ ] `cargo test -p patches-modules` passes with 0 failures.
- [ ] `cargo clippy -p patches-modules` passes with 0 warnings.

## Notes

The `make_*` helper refactors are purely mechanical moves of inline struct
literals — no behaviour changes. Confirm via `cargo test` before and after each
move.

The MAD smoothness test for `PolyNoise` should iterate over all 16 voices and
`assert!(mad_brown[v] < mad_white[v])` etc., rather than averaging across voices,
so that a single misbehaving voice index is immediately visible in the failure
message.
