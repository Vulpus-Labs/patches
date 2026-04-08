---
id: "0294"
title: "patches-wasm: scanner and registry integration"
priority: medium
created: 2026-04-08
---

## Summary

Add a `.wasm` file scanner to `patches-wasm` that discovers WASM plugins in a
directory and registers them in the module registry, analogous to
`patches-ffi`'s `scan_plugins`/`register_plugins`.

## Acceptance criteria

- [ ] `scan_wasm_plugins(engine, dir) -> Vec<Result<(String, WasmModuleBuilder), String>>` scans a directory for `.wasm` files, loads each, calls `describe` to extract the module name
- [ ] `register_wasm_plugins(engine, dir, registry) -> Vec<String>` scans and registers all successful builders; returns error messages for failures
- [ ] Broken `.wasm` files do not prevent other plugins from loading
- [ ] Registered WASM modules are indistinguishable from native modules at the registry level
- [ ] `cargo test` and `cargo clippy` clean

## Notes

- The `wasmtime::Engine` is passed in (not created per scan) so it can be
  shared with other parts of the system.
- Scanner looks for files with `.wasm` extension only.
- The caller (e.g. `patches-player`) decides when to call both
  `patches_ffi::register_plugins()` and `patches_wasm::register_wasm_plugins()`
  during startup.
- If a WASM module and a native module have the same name, the last one
  registered wins (standard HashMap behavior). Document this.

Epic: E055
ADR: 0027
Depends: 0293
