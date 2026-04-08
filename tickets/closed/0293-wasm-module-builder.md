---
id: "0293"
title: "patches-wasm: WasmModuleBuilder and WasmModule"
priority: high
created: 2026-04-08
---

## Summary

Create the `patches-wasm` crate with the host-side WASM loader. Implement
`WasmModuleBuilder` (implementing `ModuleBuilder`) and `WasmModule`
(implementing `Module`) that load and execute `.wasm` plugin modules via
wasmtime.

## Acceptance criteria

- [ ] `patches-wasm` crate exists in the workspace
- [ ] Depends on `patches-core`, `patches-ffi-common`, `wasmtime`
- [ ] `load_wasm_plugin(engine, path) -> Result<WasmModuleBuilder, String>` loads and compiles a `.wasm` file
- [ ] `WasmModuleBuilder` implements `ModuleBuilder`:
  - `describe(shape)` creates a temp Store+Instance, calls `patches_describe`, parses JSON
  - `build(env, shape, params, id)` creates Store+Instance, calls `patches_prepare`, fills defaults, applies params, returns `Box<dyn Module>`
- [ ] `WasmModule` implements `Module`:
  - `process(pool)`: copies input cable slots into WASM staging area, calls `patches_process`, copies output slots back
  - `set_ports(inputs, outputs)`: remaps host cable indices to 0-based staging indices, writes port structs into WASM memory, updates internal cable mapping
  - `update_validated_parameters(params)`: serializes to JSON, writes into WASM memory, calls export
  - `update_parameters(params)`: same with error handling
  - `descriptor()`: returns stored ModuleDescriptor
  - `instance_id()`: returns stored InstanceId
  - `as_periodic()`: delegates to `patches_supports_periodic` / `patches_periodic_update`
- [ ] Each WasmModule owns its own `wasmtime::Store` + `wasmtime::Instance`; compiled `wasmtime::Module` shared via `Arc`
- [ ] Cable staging area allocated once at build time via `patches_alloc`; reused across `process()` calls
- [ ] Function handles cached as `TypedFunc` fields on WasmModule (no per-call lookup)
- [ ] Can load and run the `test-gain-wasm-plugin` `.wasm` file: describe, build, process, parameter update
- [ ] `cargo test` and `cargo clippy` clean

## Notes

- The cable remapping in `set_ports` is critical: the WASM module sees cable
  indices 0..N in its staging area, not the host's global cable indices. The
  host maintains a `Vec<usize>` mapping staging slots → host cable indices.
- `WasmModule` must be `Send` (wasmtime Store is Send). Mark with
  `unsafe impl Send` and document the contract.
- wasmtime `Engine` should be created once and shared. Consider storing it on
  `WasmModuleBuilder` as `Arc<Engine>`.
- The `process()` copy path copies `[CableValue; 2]` entries (both ping-pong
  slots) for inputs, but only the write slot `pool[idx][wi]` for outputs.
  Actually, copy both slots for inputs and read back only the write slot for
  outputs.

Epic: E055
ADR: 0027
Depends: 0290, 0292
