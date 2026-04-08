---
id: "0137"
title: Migrate patches-modules unit tests to ModuleHarness and new macros
priority: medium
created: 2026-03-18
---

## Summary

Replace per-module test scaffolding in `patches-modules` with `ModuleHarness`,
`assert_nearly!`, `assert_within!`, and `params!` introduced in T-0135 and T-0136.

## Acceptance criteria

- [ ] All `make_<module>` factory functions in `patches-modules` test modules are removed
      and replaced with `ModuleHarness::build::<M>(...)`.
- [ ] All `make_pool` helper functions are removed.
- [ ] All `set_ports_for_test` / `set_ports_outputs_only` / `set_ports_none_connected`
      helper functions are removed. Tests that require a subset of ports connected
      use `ModuleHarness::disconnect` (to be added in T-0135 if needed) or an equivalent
      harness-level API rather than constructing `InputPort`/`OutputPort` slices directly.
- [ ] All `assert!((expected - actual).abs() < ...)` patterns are replaced with
      `assert_nearly!` or `assert_within!` as appropriate.
- [ ] Manual ping-pong loops (`for i in 0..n { let wi = i % 2; ... }`) are replaced
      with `harness.run_mono(n, "output_name")` or equivalent.
- [ ] No test loses coverage: all assertions present before migration are present after.
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no warnings across all crates.

## Notes

Migrate one module at a time and commit per-module to keep diffs reviewable. A suggested
order (simplest first): Vca → Sum → Glide → ResonantLowpass → ResonantHighpass →
ResonantBandpass → Oscillator → Lfo → remaining modules.

Tests that need to check connectivity-dependent behaviour (e.g. that an oscillator
produces a static output when `voct` is not connected) may need `ModuleHarness::disconnect`
or a lower-level workaround. Note these as they arise and extend the harness in T-0135 if
the pattern is common enough.

Do not migrate integration tests in `patches-integration-tests`. Those use `HeadlessEngine`
and test multi-module behaviour; `ModuleHarness` is not appropriate there.
