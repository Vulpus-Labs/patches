---
id: "0296"
title: "patches-wasm: integration tests"
priority: high
created: 2026-04-08
---

## Summary

Write integration tests for the WASM module loader, mirroring the test
structure in `patches-ffi/tests/gain_plugin.rs`. Load the `test-gain-wasm-plugin`
`.wasm` file and exercise the full round-trip: describe, build, process,
parameter update, validation, drop.

## Acceptance criteria

- [ ] `patches-wasm/tests/gain_wasm_plugin.rs` exists with integration tests
- [ ] Test: `describe_returns_correct_metadata` — loads `.wasm`, calls describe, verifies module name, port names, parameter ranges
- [ ] Test: `build_and_process_with_default_gain` — builds a WasmModule, wires up a CablePool, processes one sample, verifies output = input × 1.0
- [ ] Test: `update_parameters_changes_gain` — changes gain to 0.5, processes, verifies output = input × 0.5
- [ ] Test: `parameter_validation_rejects_out_of_range` — attempts gain = 3.0 (max 2.0), verifies error
- [ ] Test: `multiple_instances_from_same_plugin` — creates two WasmModule instances from the same `.wasm`, verifies they have independent state
- [ ] All tests find the `.wasm` file at `target/wasm32-unknown-unknown/debug/test_gain_wasm_plugin.wasm` (or use a build script / env var)
- [ ] `cargo test -p patches-wasm` passes

## Notes

- Tests require `cargo build --target wasm32-unknown-unknown -p test-gain-wasm-plugin`
  to have been run first. Document this prerequisite. Consider a build script
  that checks for the `.wasm` file and prints a helpful error.
- The test CablePool setup should match the pattern used in
  `patches-ffi/tests/gain_plugin.rs` — allocate a small buffer pool, set up
  input/output cables, call set_ports, then process.
- Drop test: verify that dropping a WasmModule does not panic or leak. Since
  WASM modules are sandboxed, this is mainly about ensuring the wasmtime Store
  is cleaned up correctly.

Epic: E055
ADR: 0027
Depends: 0292, 0293
