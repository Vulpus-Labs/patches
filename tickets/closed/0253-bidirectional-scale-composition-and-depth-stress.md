---
id: "0253"
title: Bidirectional scale composition and depth stress
priority: low
created: 2026-04-02
---

## Summary

Scale composition tests cover in-boundary and out-boundary scales separately,
and test three nesting levels. No test combines non-trivial scales on *both*
the in-boundary and out-boundary of the same template simultaneously. No test
verifies that deeper nesting (beyond 3 levels) works correctly.

## Acceptance criteria

- [ ] **Bidirectional scale:** A template with a non-trivial in-boundary scale
      (e.g. 0.4 on `sink.voct <-[0.4]- $.x`) AND a non-trivial out-boundary
      scale (e.g. 0.6 on `$.y <-[0.6]- sink.out`) in the same template. Outer
      connections also carry non-trivial scales. Assert the composed scale on
      the through-path is the product of all four factors.
- [ ] **Depth stress at 10 levels:** Programmatically generate (or write by
      hand) a fixture with 10 nesting levels, each carrying a known scale.
      Assert the final composed scale matches the expected product. This
      exercises the recursion depth without being so deep as to be fragile.

## Notes

- The bidirectional test is the more important of the two — it catches a class
  of bug where the expander only applies scale composition on one direction of
  boundary crossing.
- The 10-level test can be an inline string built with `format!` in a loop to
  avoid a huge hand-written fixture.
- Epic: E046
