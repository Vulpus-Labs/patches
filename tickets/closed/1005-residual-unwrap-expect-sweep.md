---
id: "1005"
title: "Residual unwrap/expect sweep to `expect_invariant`"
priority: low
created: 2026-06-11
---

## Summary

Ticket 1000 fixed the curated high-value `unwrap`/`expect` violations and
introduced the sanctioned `patches_core::ExpectInvariant` mechanism. A
fresh grep still finds ~100 library-code sites outside tests, dominated by
two accepted-but-unmigrated classes:

- **Grammar-guaranteed parser internals** (~80 sites in
  `patches-dsl/src/parser/*`): `pair.into_inner().next().unwrap()` where
  the PEG grammar guarantees the child exists. Mostly already commented
  `// grammar guarantees one child`.
- **Layout / algorithm invariants** (~20 sites): e.g.
  `patches-ffi-common/src/port_frame.rs` overflow guards,
  `patches-planner/src/state/scc.rs` Tarjan stack, `params_enum.rs`
  "enum has ≥1 variant".

None are user-reachable bugs; they're documented invariants. This ticket
migrates them to `expect_invariant` (or `assert!`) so the policy's
"`\.unwrap(`/`\.expect(` greps return only real violations" goal is fully
realised.

## Acceptance criteria

- [ ] Parser `into_inner().next().unwrap()` sites converted to
      `expect_invariant("grammar guarantees …")` or restructured.
- [ ] Remaining layout/algorithm invariant sites converted.
- [ ] `self.expect(byte)` in `patches-ffi-common/src/json/de.rs` is a
      custom method, not std `expect` — confirm and leave (or rename to
      avoid grep noise).
- [ ] Fresh grep over library `src` (excluding tests) returns only
      `expect_invariant` / custom-method hits.

## Notes

Low priority: purely a greppability/consistency cleanup, no behaviour
change. Spun out of 1000 to keep that ticket's diff proportionate to its
curated site list.
