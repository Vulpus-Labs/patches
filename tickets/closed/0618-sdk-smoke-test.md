---
id: "0618"
title: SDK smoke test — minimal in-crate Module round-trip via macro
priority: high
created: 2026-04-21
---

## Summary

Inline test inside `patches-ffi-common` that pretends to be a
plugin: invokes `export_plugin!` on a trivial Module, then
calls the generated symbols directly (no dylib) with a
host-built `ParamFrame` and asserts the module sees the
expected values.

## Acceptance criteria

- [ ] Test covers every scalar tag + one buffer id.
- [ ] Test calls `destroy` and asserts the instance drops.
- [ ] Runs under `cargo test -p patches-ffi-common`.

## Notes

Epic E105. This is the guard that proves the macro expansion
works before a real cdylib enters the picture.
