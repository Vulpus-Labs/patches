---
id: "0620"
title: Delete conv-reverb, drums-bundle, gain-wasm, old-abi test plugins
priority: medium
created: 2026-04-21
---

## Summary

The FFI subsystem has no external users. The four extra test
plugin crates under `test-plugins/` bind us to the old ABI or
orthogonal concerns (wasm, deprecation fixtures). Delete them.

## Acceptance criteria

- [ ] `test-plugins/conv-reverb/`, `test-plugins/drums-bundle/`,
      `test-plugins/gain-wasm/`, `test-plugins/old-abi/`
      directories removed.
- [ ] Workspace `Cargo.toml` members list updated.
- [ ] Any integration tests that referenced these plugins
      removed or rewritten against `gain`.
- [ ] `cargo build --workspace` clean.

## Notes

Epic E106. If any of these plugins had unique test scenarios
worth preserving (e.g. convolution reverb as a stress test for
buffer-handle lifecycle), note them in a followup ticket —
don't try to salvage inside this one.
