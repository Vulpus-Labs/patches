---
id: "0488"
title: Extract tests from patches-core test_support/harness.rs
priority: low
created: 2026-04-16
---

## Summary

`patches-core/src/test_support/harness.rs` is 898 lines, of which
270 (30%) are the inline test module (self-tests for the harness).
Extract to a sibling `harness/tests.rs`.

## Acceptance criteria

- [ ] `test_support/harness.rs` → `test_support/harness/mod.rs` +
      `test_support/harness/tests.rs`.
- [ ] Parent declares `#[cfg(test)] mod tests;`.
- [ ] `cargo test -p patches-core` unchanged.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

E085. Mechanical extraction, no behaviour change. Note that the
harness itself is a test utility; the `#[cfg(test)] mod tests` is
the harness's self-tests, not tests of code under test.
