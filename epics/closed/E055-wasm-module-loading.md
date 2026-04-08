# E055 — WASM module loading

## Goal

Enable loading of audio modules from `.wasm` files, providing sandboxed,
cross-platform, polyglot plugin support alongside the existing native C ABI
plugin system (E052).

After this epic:

- A `patches-ffi-common` crate holds the shared JSON serialization and repr(C)
  types used by both native and WASM plugin loading.
- A `patches-wasm` crate loads `.wasm` files via wasmtime and presents them
  through the `ModuleBuilder` trait — indistinguishable from native modules at
  the registry level.
- A `patches-wasm-sdk` crate provides Rust plugin authors with an
  `export_wasm_module!` macro for targeting `wasm32-unknown-unknown`.
- A test Gain plugin compiles to WASM and passes the same integration tests as
  the native Gain plugin.

## Background

ADR 0027 documents the design. Key decisions:

- **Memory model**: copy-in/copy-out with cable remapping. Only used cable
  slots are transferred. ~476 bytes memcpy + ~100ns call overhead per sample
  for a typical module.
- **Instance isolation**: each WasmModule gets its own wasmtime Store+Instance.
  Compiled code shared via Arc.
- **AOT caching**: compiled WASM serialized to `.wasmcache` for fast reload.
- **Multi-language**: the WASM export contract is language-agnostic. Rust is
  the initial target; C/C++, Zig, and AssemblyScript are natural follow-ons
  requiring only plugin-side SDKs.

## Tickets

| ID   | Title                                            | Dependencies |
|------|--------------------------------------------------|--------------|
| 0290 | Extract patches-ffi-common from patches-ffi      | —            |
| 0291 | patches-wasm-sdk: export macro and CablePool shim | 0290        |
| 0292 | Test plugin: gain-wasm                           | 0291         |
| 0293 | patches-wasm: WasmModuleBuilder and WasmModule   | 0290, 0292   |
| 0294 | patches-wasm: scanner and registry integration   | 0293         |
| 0295 | patches-wasm: AOT compilation cache              | 0293         |
| 0296 | patches-wasm: integration tests                  | 0293, 0292   |

Epic: E055
ADR: 0027
