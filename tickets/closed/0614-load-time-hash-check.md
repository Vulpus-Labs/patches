---
id: "0614"
title: Load-time descriptor_hash check; refuse mismatch
priority: high
created: 2026-04-21
---

## Summary

At plugin load the host computes `descriptor_hash` from its
`ModuleDescriptor`, calls the plugin's exported
`descriptor_hash()` symbol, compares. Mismatch = fatal error;
plugin does not initialise, no `create` call, dylib unloaded.

## Acceptance criteria

- [ ] Loader reads `descriptor_hash: extern "C" fn() -> u64`
      symbol from the dylib during `DylibModuleBuilder`
      construction.
- [ ] Mismatch produces a descriptive error listing module name,
      host hash, plugin hash.
- [ ] On mismatch the `libloading::Library` is dropped — no
      plugin entry points called.
- [ ] Unit test with a stub plugin whose hash is manually
      corrupted: load fails, no side effects.

## Notes

Epic E104. Stub plugin for the test can live under
`patches-ffi/tests/fixtures/`.
