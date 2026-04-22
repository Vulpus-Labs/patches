---
id: "0571"
title: Integration test — PluginScanner loads patches-vintage bundle
priority: high
created: 2026-04-19
epic: "E095"
---

## Summary

Add a `patches-integration-tests` test that builds `patches-vintage`
as a cdylib, runs `PluginScanner` against the build output, and
asserts every expected module name, version, and a shared library
refcount.

## Acceptance criteria

- [ ] Test builds or locates the `patches-vintage` dylib in the
      workspace target dir (use `CARGO_BIN_EXE_*`-style discovery or a
      build-script helper; document the approach).
- [ ] `PluginScanner { paths: vec![dylib_path] }.scan(&mut registry)`
      returns a `ScanReport` whose `loaded` set includes every public
      vintage module name.
- [ ] Each loaded entry has `module_version > 0`.
- [ ] Constructing two modules from the bundle and asserting their
      `Arc<Library>` refcount > 1 (extends 0496 style).
- [ ] A simple smoke-processing pass on a VChorus instance produces
      finite output (reuses existing unit-test fixture if possible).

## Notes

Gate: test is skipped (not failed) if the dylib is absent, to keep
clean `cargo test` on minimal builds. Document the convention.
