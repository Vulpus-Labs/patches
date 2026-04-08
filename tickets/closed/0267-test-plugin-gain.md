---
id: "0267"
title: "Test plugin: Gain cdylib"
priority: high
created: 2026-04-07
---

## Summary

Build a minimal Gain module as a `cdylib` plugin to validate the full FFI
round-trip: load library, describe, build, process, update parameters, drop.
This is the first end-to-end test of the plugin system.

## Acceptance criteria

- [ ] A `test-plugins/gain/` directory with `Cargo.toml` (crate-type = ["cdylib"]) and `src/lib.rs`
- [ ] `Gain` module: one mono input, one mono output, one float parameter `"gain"` (0.0..2.0, default 1.0), `process` multiplies input by gain
- [ ] Uses `export_module!(Gain)` to generate the plugin entry point
- [ ] Integration test (in `patches-ffi/tests/` or `patches-integration-tests/`) that:
  - Loads the compiled `.dylib` via `load_plugin`
  - Calls `describe` and verifies module name, port count, parameter descriptor
  - Calls `build` with default parameters
  - Runs `process` for N samples with known input, verifies output = input * 1.0
  - Calls `update_parameters` with gain = 0.5, processes again, verifies output = input * 0.5
  - Drops the `DylibModule`, no crash or leak
- [ ] Integration test for ABI version mismatch rejection
- [ ] Integration test for parameter validation error (e.g. gain = 3.0 out of range) — error propagates correctly across FFI boundary
- [ ] `cargo clippy` clean on both the test plugin and the host-side tests

## Notes

- The test must build the cdylib first. This can be done via `cargo build -p
  test-gain-plugin` in a build script or test fixture, or by adding the test
  plugin crate to the workspace and relying on workspace-level `cargo build`.
- The `.dylib` path can be determined from `CARGO_TARGET_DIR` / target triple /
  debug or release.
- This is deliberately simple — no threads, no I/O, no PeriodicUpdate. The
  goal is to validate the FFI plumbing before testing complex modules.

Epic: E052
ADR: 0025
Depends: 0265, 0266
