---
id: "E027"
title: Module test harness
created: 2026-03-18
tickets: ["0135", "0136", "0137"]
---

## Summary

Unit tests for module implementations carry a large amount of structural setup —
`Registry` construction, pool sizing, manual `InputPort`/`OutputPort` construction,
ping-pong index management, and `CableValue` unwrapping — that is unrelated to the
behaviour under test. ADR 0018 specifies a `ModuleHarness` struct, a `params!` macro,
and `assert_nearly!`/`assert_within!` assertion macros that eliminate this scaffolding
and let tests express only their intent.

## Tickets

| ID   | Title                                                            | Priority | Depends on |
|------|------------------------------------------------------------------|----------|------------|
| 0135 | Add `ModuleHarness` to `patches-core` (`test-support` feature)  | high     | —          |
| 0136 | Add `assert_nearly!`, `assert_within!`, and `params!` macros    | medium   | —          |
| 0137 | Migrate `patches-modules` unit tests to harness and new macros  | medium   | 0135, 0136 |

## Definition of done

- `ModuleHarness` lives in `patches-core/src/test_support/harness.rs` behind
  `#[cfg(any(test, feature = "test-support"))]`.
- `ModuleHarness::build::<M>(params)` constructs a module, derives port-to-cable
  mappings from the descriptor, allocates a correctly-sized pool, and calls
  `set_ports` — all ports connected by default.
- `set_mono` / `set_mono_at`, `tick`, `read_mono` / `read_mono_at`, `run_mono`,
  and `run_mono_mapped` are implemented. Poly equivalents (`set_poly`, `read_poly`,
  `run_poly_mapped`) are implemented.
- `assert_nearly!(expected, actual)` uses a relative epsilon scaled by
  `expected.abs().max(1.0)`.
- `assert_within!(expected, actual, delta)` uses an absolute tolerance.
- `params![...]` macro infers `ParameterValue` variant from literal type
  (`f32` → `Float`, `bool` → `Bool`, `&str` → `Enum`, `i64` → `Int`).
- All three are re-exported from `patches_core::test_support`.
- All unit tests in `patches-modules` are migrated to use `ModuleHarness` and the
  new assertion macros. Per-module factory functions and `set_ports_for_test` helpers
  are removed.
- `cargo build`, `cargo test`, `cargo clippy` pass with no warnings across all crates.
- No `unwrap()` or `expect()` in library code.
