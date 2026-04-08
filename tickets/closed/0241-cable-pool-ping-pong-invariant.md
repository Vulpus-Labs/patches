---
id: "0241"
title: Add cable_pool ping-pong 1-sample-delay invariant test
priority: high
created: 2026-04-01
---

## Summary

The cable pool's ping-pong buffer scheme is the execution model's defining
invariant: a module reading an input sees the value written *last* tick, not
this tick. This property enables order-independent module execution and is the
foundation for future parallelism. It currently has no explicit test.

## Acceptance criteria

- [ ] Test in `patches-core/src/cable_pool.rs` (or `cables.rs`) that
      demonstrates the 1-sample delay: write a value at tick N, read at tick N
      returns the *previous* value, read at tick N+1 returns the written value.
- [ ] Test that two modules writing and reading the same cable slot on the same
      tick do not see each other's writes (isolation within a tick).
- [ ] Test that `scale` is applied at read time, not write time: write `v`,
      read with scale `s` → result is `v * s`.
- [ ] Test the scale=1.0 fast path produces the same result as the general path
      (code has an optimisation branch at `cable_pool.rs:73-76`).

## Notes

These tests exercise `CablePool::new()`, `read_mono()`, `write_mono()`,
`read_poly()`, `write_poly()` directly — no module harness needed. Use raw
pool arrays and explicit write-index management.
