---
id: "0627"
title: CI grep gate — no JSON / no alloc on FFI audio path
priority: medium
created: 2026-04-21
---

## Summary

Add a CI step (shell script or cargo xtask) that greps the
three FFI audio entry points' call graphs for forbidden
identifiers. Cheap tripwire for future regressions.

## Acceptance criteria

- [ ] Script asserts: no `json::`, no `Vec::new`/`Vec::with_capacity`,
      no `Box::new`, no `String::` on lines between
      `fn update_validated_parameters` and its closing brace
      (and likewise for `set_ports`, `process`) in
      `patches-ffi/src/loader.rs`.
- [ ] Script lives under `tools/` and runs in CI.
- [ ] Failing the grep fails the build.

## Notes

Epic E108. Not a full call-graph analysis — a naive
intra-function grep is sufficient because the ABI functions
are small and the real no-alloc enforcement is the allocator
trap.
