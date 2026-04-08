---
id: "0157"
title: Strengthen default_registry_contains_all_modules and fix ADSR unused variable
epic: E026
priority: low
created: 2026-03-20
---

## Summary

Two test quality issues:

1. `default_registry_contains_all_modules` in `patches-modules/src/lib.rs` (lines 82-120) instantiates every module with dummy parameters and asserts only that it doesn't crash. A module that silently returns zeros for all outputs would still pass. The test doesn't verify any meaningful output.

2. `patches-modules/src/adsr.rs` (line 236) uses `for (i, &exp) in expected.iter().enumerate()` where `i` is never used. This generates a compiler warning and suggests the test was copied from code that used the index.

## Acceptance criteria

- [ ] `default_registry_contains_all_modules` is extended to run at least one tick on a representative module (e.g. `Oscillator` or `SineOscillator`) with meaningful parameters and assert that at least one output is non-zero after the tick.
- [ ] The ADSR test loop is changed to `for &exp in expected.iter()` (removing the unused `i`), or the index is used meaningfully.
- [ ] No existing tests are removed or weakened.
- [ ] `cargo clippy` clean (the unused variable warning should disappear); `cargo test` passes.
