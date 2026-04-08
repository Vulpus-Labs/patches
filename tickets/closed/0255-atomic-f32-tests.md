---
id: "0255"
title: AtomicF32 round-trip and ordering tests
priority: low
created: 2026-04-02
---

## Summary

`AtomicF32` is a lock-free f32 wrapper using `AtomicU32` with bit-casting. It
currently has zero tests. While it is a thin wrapper, a round-trip test and a
basic ordering test would catch any bit-casting errors introduced by future
changes.

## Acceptance criteria

- [ ] Test that `store` followed by `load` returns the original value for normal
      floats (0.0, 1.0, -1.0, PI, very small, very large).
- [ ] Test round-trip with special values: subnormals, infinity, negative zero.
      NaN requires `to_bits()` comparison since NaN != NaN.
- [ ] Test that `new(x).load()` returns `x`.
- [ ] Test that `Default` produces 0.0.
- [ ] All tests are unit tests in `atomic_f32.rs`.

## Notes

This is trivial work but closes a gap where a zero-test module is exported as
part of the public API.
