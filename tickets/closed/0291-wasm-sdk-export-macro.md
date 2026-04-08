---
id: "0291"
title: "patches-wasm-sdk: export macro and CablePool shim"
priority: high
created: 2026-04-08
---

## Summary

Create the `patches-wasm-sdk` crate — the plugin-side authoring SDK for WASM
modules targeting `wasm32-unknown-unknown`. Provides an `export_wasm_module!`
macro and a WASM-side `CablePool` shim that reads/writes the cable staging area
in WASM linear memory.

## Acceptance criteria

- [ ] `patches-wasm-sdk` crate exists in the workspace
- [ ] Depends on `patches-core` and `patches-ffi-common`
- [ ] Compiles for `wasm32-unknown-unknown`
- [ ] `export_wasm_module!(ModuleType)` macro generates all WASM export functions:
  - `patches_describe(channels: i32, length: i32, hq: i32) -> i32`
  - `patches_prepare(desc_ptr: i32, desc_len: i32, sample_rate: f32, poly_voices: i32, periodic_interval: i32, instance_id_lo: i32, instance_id_hi: i32)`
  - `patches_process(cable_ptr: i32, cable_count: i32, write_index: i32)`
  - `patches_set_ports(inputs_ptr: i32, inputs_len: i32, outputs_ptr: i32, outputs_len: i32)`
  - `patches_update_validated_parameters(params_ptr: i32, params_len: i32)`
  - `patches_update_parameters(params_ptr: i32, params_len: i32) -> i32`
  - `patches_periodic_update(cable_ptr: i32, cable_count: i32, write_index: i32) -> i32`
  - `patches_supports_periodic() -> i32`
  - `patches_alloc(size: i32) -> i32`
  - `patches_free(ptr: i32, size: i32)`
- [ ] WASM-side `CablePool` shim provides `read_mono`, `write_mono`, `read_poly`, `write_poly` operating on the staging area pointer
- [ ] Module singleton stored as `static mut` (safe: WASM is single-threaded)
- [ ] Return values (JSON bytes) use a length-prefixed convention: `patches_alloc` for the buffer, write `[len: u32, data...]`, return pointer
- [ ] `cargo check --target wasm32-unknown-unknown -p patches-wasm-sdk` passes
- [ ] `cargo clippy` clean

## Notes

- The WASM export functions mirror the `FfiPluginVTable` function pointers from
  `patches-ffi`, adapted for WASM's flat i32/f32 calling convention.
- `instance_id` is split into `lo`/`hi` i32 halves because WASM MVP does not
  support i64 parameters in all runtimes (wasmtime does, but splitting is more
  portable).
- The `patches_alloc`/`patches_free` exports let the host write data (JSON,
  port arrays) into WASM linear memory.
- No global allocator is specified — the plugin crate provides one (e.g.
  `wee_alloc` or the default WASM allocator).

Epic: E055
ADR: 0027
Depends: 0290
