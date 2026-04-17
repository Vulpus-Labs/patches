---
id: "0512"
title: Extract patches-registry crate from patches-core
priority: high
created: 2026-04-17
---

## Summary

Move `patches-core/src/registries/` into a new `patches-registry`
crate. `patches-core` becomes registry-agnostic; every consumer that
needs registration, module lookup, or plugin loading depends on
`patches-registry` directly.

Part of epic E089 (see ADR 0040).

## Acceptance criteria

- [ ] New `patches-registry/` crate exists with `publish = false`.
- [ ] Files moved: `registry.rs`, `module_builder.rs`,
  `file_processor.rs`, and `mod.rs` content from
  `patches-core/src/registries/`.
- [ ] `patches-registry` depends on `patches-core` (for `Module`,
  `ModuleDescriptor`, `ParameterMap`, `AudioEnvironment`, `BuildError`).
  `patches-core` does not depend on `patches-registry`.
- [ ] `patches-core` no longer exposes `registries` module or
  `Registry` type; `grep -r 'patches_core::Registry' --type rust` is
  empty.
- [ ] Consumers updated: `patches-engine`, `patches-interpreter`,
  `patches-lsp`, `patches-clap`, `patches-player`, `patches-modules`,
  `patches-ffi`, `patches-ffi-common`, `patches-wasm`,
  `patches-integration-tests`, `patches-svg`.
- [ ] `patches-modules::default_registry()` returns
  `patches_registry::Registry`.
- [ ] FFI / WASM plugin builders (`DylibModuleBuilder`,
  `WasmModuleBuilder`) implement `patches_registry::ModuleBuilder`.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean across the
  workspace.

## Notes

No behaviour change. This is an import-path refactor plus crate
boundary relocation.

The ten existing consumers already take `&Registry` by reference
rather than constructing their own, so the change is almost purely
`use patches_core::registries::...` → `use patches_registry::...`.

Confirm no dependency cycle: run `cargo tree -p patches-core` and
verify `patches-registry` is absent.
