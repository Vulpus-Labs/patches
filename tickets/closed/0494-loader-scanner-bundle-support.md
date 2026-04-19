---
id: "0494"
title: Loader and scanner support for multi-module bundles
priority: high
created: 2026-04-16
---

## Summary

Update `load_plugin` in `patches-ffi/src/loader.rs` to read an
`FfiPluginManifest` and return `Vec<DylibModuleBuilder>` (one builder
per vtable, all sharing one `Arc<libloading::Library>`). Update
`scan_plugins` and `register_plugins` in
`patches-ffi/src/scanner.rs` to flatten across bundles. Reject ABI
v1 plugins with a clear error.

## Acceptance criteria

- [ ] `load_plugin(path) -> Result<Vec<DylibModuleBuilder>, String>` reads the manifest, validates `abi_version == 2`, and constructs one `DylibModuleBuilder` per vtable, all cloning a single `Arc<Library>`.
- [ ] Within one manifest, duplicate `module_name` (from the per-vtable `describe()` call) is a load-time error reported as a single error string for the whole file.
- [ ] `scan_plugins(dir)` returns `Vec<Result<(String, DylibModuleBuilder), String>>` with each module from each bundle as a separate entry; per-file load errors continue to be one entry per file.
- [ ] `register_plugins` registers every successful builder; existing collision policy on the `Registry` side is preserved.
- [ ] Existing scanner tests (`scan_nonexistent_directory`, `scan_empty_directory`) updated and passing.
- [ ] New test: a fixture manifest with two trivial vtables loads as two builders sharing one `Arc<Library>` (assert `Arc::strong_count >= 2`).
- [ ] ABI v1 plugin (simulated by a manifest with `abi_version: 1`) yields a load error mentioning the version mismatch.
- [ ] `patches-player` and any other `load_plugin` consumers updated to handle the vec return type.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean across the workspace.

## Notes

ADR 0039. The drop-order contract from ADR 0025 is preserved: each
`DylibModule` declares `handle` and `vtable` before `_lib`, so
`vtable.drop` runs before the `Arc<Library>` decrement. Multiple
modules in a bundle simply hold separate clones of the same `Arc`.
