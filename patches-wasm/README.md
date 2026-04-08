# patches-wasm

> **Experimental / shelved.** This crate was built as part of an implementation
> spike and is not part of the active build. It may not compile against the
> current `patches-core` API. See [ADR 0027](../adr/0027-wasm-module-loading.md)
> for design rationale and spike findings.

Host-side WASM module loader using wasmtime. Provides `WasmModuleBuilder`
(implements `ModuleBuilder`), `WasmModule` (implements `Module`), a `.wasm` file
scanner, and an AOT compilation cache.
