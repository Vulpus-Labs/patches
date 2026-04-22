---
id: "0619"
title: Rewrite test-plugins/gain on export_plugin! + new ABI
priority: high
created: 2026-04-21
---

## Summary

`test-plugins/gain/src/lib.rs` currently uses the old ABI. Strip
it down to a `Module` impl + `export_plugin!` invocation. The
crate becomes ~30 LoC.

## Acceptance criteria

- [ ] `test-plugins/gain/src/lib.rs` contains: Module struct,
      trait impl, descriptor fn, `export_plugin!` invocation.
      Nothing else.
- [ ] `cargo build -p gain --release` produces a `.dylib`.
- [ ] `patches-ffi` loader loads the dylib; descriptor hash
      matches.
- [ ] End-to-end: `patches-player` loads a patch referencing
      the gain bundle, processes 1 s of input, output matches
      reference WAV bit-identically (golden captured pre-
      migration from the old path at matching parameter values).

## Notes

Epic E106. Reference WAV capture step: before starting the
rewrite, run the existing gain dylib through a known input and
stash the output under `test-plugins/gain/tests/fixtures/`.
