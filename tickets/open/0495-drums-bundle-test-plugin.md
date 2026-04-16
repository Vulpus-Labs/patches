---
id: "0495"
title: drums-bundle test plugin
priority: medium
created: 2026-04-16
---

## Summary

Add a new test plugin crate under `test-plugins/drums-bundle/` that
re-exports the eight existing drum modules from `patches-modules`
(`Kick`, `Snare`, `ClapDrum`, `ClosedHiHat`, `OpenHiHat`, `Tom`,
`Claves`, `Cymbal`) via a single `export_modules!(...)` invocation.
This validates the bundle ABI on the real DSP-sharing case that
motivated it.

## Acceptance criteria

- [ ] `test-plugins/drums-bundle/Cargo.toml` declares `crate-type = ["cdylib"]` and depends on `patches-core`, `patches-ffi`, `patches-modules` (the drum types are already public).
- [ ] `src/lib.rs` is essentially `patches_ffi::export_modules!(Kick, Snare, ClapDrum, ClosedHiHat, OpenHiHat, Tom, Claves, Cymbal);` plus the necessary `use` statements.
- [ ] `cargo build -p drums-bundle` produces a single dylib.
- [ ] Loading the dylib via `load_plugin` returns 8 builders; each `describe()` returns the expected module name.
- [ ] Each loaded module passes a smoke test: build with default params, deliver one trigger, observe non-zero output for at least 100 samples (mirrors the existing per-module trigger-response tests in `patches-modules`).
- [ ] No new dependencies added beyond what's already in the workspace.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

E088. This crate exists alongside `patches-modules` registering the
same drums in `default_registry()` — the bundle is for validating the
plugin path, not a replacement. Whether to remove drums from
`default_registry()` and ship them only as a bundle is deferred.
