---
id: "0569"
title: Convert patches-vintage to cdylib bundle with export_modules!
priority: high
created: 2026-04-19
epic: "E095"
---

## Summary

Rebuild `patches-vintage` as a multi-module FFI bundle. One `.dylib`
exports every public vintage module (VChorus, BBD, VFlanger,
VFlangerStereo, compander, …) through `export_modules!` with per-module
version symbols.

## Acceptance criteria

- [ ] `patches-vintage/Cargo.toml` sets `crate-type = ["cdylib", "rlib"]`.
- [ ] Existing rlib tests continue to compile and pass unchanged.
- [ ] A single `export_modules!` invocation enumerates every vintage
      module with a `module_version` per entry.
- [ ] ABI version symbol present and matches host workspace ABI.
- [ ] `cargo build -p patches-vintage` produces a `.dylib` / `.so` /
      `.dll` in the target dir.
- [ ] Unit test (within the bundle) sanity-checks manifest contents
      via the crate's rlib side.

## Notes

Depends on E088 (bundle ABI v2, `export_modules!` macro) and ticket
0563 (version symbol).
