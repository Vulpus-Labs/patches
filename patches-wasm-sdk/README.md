# patches-wasm-sdk

> **Experimental / shelved.** This crate was built as part of an implementation
> spike and is not part of the active build. It may not compile against the
> current `patches-core` API. See [ADR 0027](../adr/0027-wasm-module-loading.md)
> for design rationale and spike findings.

Rust SDK for authoring WASM audio modules. Provides the `export_wasm_module!`
macro, which generates all WASM export functions for a `Module` implementation.
