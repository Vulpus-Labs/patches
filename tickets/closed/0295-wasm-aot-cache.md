---
id: "0295"
title: "patches-wasm: AOT compilation cache"
priority: low
created: 2026-04-08
---

## Summary

Add ahead-of-time compilation caching to `patches-wasm` so that WASM plugins
are compiled once and reused on subsequent loads, avoiding the ~100ms+
compilation cost on every startup.

## Acceptance criteria

- [ ] `load_or_compile(engine, wasm_path) -> Result<wasmtime::Module, String>` checks for a cached compiled artifact before compiling from source
- [ ] Cache file stored alongside the `.wasm` file as `<name>.wasmcache`
- [ ] Cache is used when: `.wasmcache` exists, is newer than the `.wasm` file, and deserializes successfully
- [ ] Cache is regenerated when: `.wasmcache` is missing, stale, or corrupt
- [ ] Wasmtime version changes automatically invalidate the cache (wasmtime embeds a version check in serialized modules)
- [ ] Cache write failures are non-fatal (best-effort caching)
- [ ] `WasmModuleBuilder` and scanner use `load_or_compile` instead of direct `Module::from_file`
- [ ] `cargo test` and `cargo clippy` clean

## Notes

- `wasmtime::Module::serialize()` returns bytes that can be written to disk.
- `unsafe { wasmtime::Module::deserialize(engine, bytes) }` loads the cached
  module. This is unsafe because a corrupted cache could cause UB — mitigated
  by wasmtime's internal version and checksum validation.
- This is an optimisation ticket. The system works without it (just slower
  startup).

Epic: E055
ADR: 0027
Depends: 0293
