---
id: "0493"
title: export_modules! macro and export_module! shim
priority: high
created: 2026-04-16
---

## Summary

Add an `export_modules!(T1, T2, ...)` macro in `patches-ffi/src/export.rs`
that registers any number of `Module` types in one bundle. Rewrite
`export_module!(T)` as a thin shim that calls `export_modules!(T)`,
preserving source compatibility for existing single-module plugins
(`gain`, `conv-reverb`, `gain-wasm`).

## Acceptance criteria

- [ ] `export_modules!` generates one set of `__patches_ffi_*::<T>` wrappers per type, a `static` `[FfiPluginVTable; N]`, and a `patches_plugin_init() -> FfiPluginManifest` referencing the static.
- [ ] `export_module!(T)` becomes `$crate::export_modules!($T);` (or equivalent) — no other change to its public contract.
- [ ] `gain`, `conv-reverb`, and `gain-wasm` test plugins compile unchanged and pass their existing tests.
- [ ] Unit test (in `patches-ffi`): expand `export_modules!(A, B)` against two trivial test modules, call the generated `patches_plugin_init`, assert `count == 2` and that each vtable's `describe` returns the expected module name.
- [ ] `catch_unwind` wrapping preserved on every generated `extern "C"` function.
- [ ] `cargo build`, `cargo clippy` clean.

## Notes

ADR 0039. The `supports_periodic` flag and panic-handling behaviour
carry over from `export_module!` per type — set per vtable, not
per bundle.
