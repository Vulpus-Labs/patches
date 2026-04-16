---
id: "0496"
title: Integration test — bundle shares one library handle
priority: medium
created: 2026-04-16
---

## Summary

Add an integration test in `patches-integration-tests/` that loads
the `drums-bundle` dylib, instantiates two distinct modules from it
(e.g. `Kick` and `Snare`), and verifies they share a single
`Arc<libloading::Library>`. This locks in the DSP-sharing guarantee
that motivated ADR 0039.

## Acceptance criteria

- [ ] Test builds `drums-bundle` as a prerequisite (via `cargo` invocation in a build script, or by relying on workspace build order — match existing patterns for `conv-reverb` integration testing).
- [ ] Test calls `load_plugin` on the produced dylib, asserts at least 8 builders returned.
- [ ] Test instantiates two modules from two different builders in that result, then asserts both `DylibModule`s outlive `Arc::strong_count >= 3` on the underlying library handle (the two modules + the original builders' references).
- [ ] Dropping all instances and builders releases the library (verifiable by `Arc::strong_count == 1` on a retained weak/clone, or by structural inspection — pick whichever is least hacky given the public API).
- [ ] No `unwrap()`/`expect()` outside the test body itself.
- [ ] `cargo test -p patches-integration-tests` passes.

## Notes

E088. If `Arc::strong_count` introspection isn't ergonomically
exposed by `DylibModuleBuilder`, add a `pub fn library_arc(&self) ->
Arc<libloading::Library>` accessor (or a `library_strong_count()` ->
`usize` helper) — minimal API surface, only used in tests.
