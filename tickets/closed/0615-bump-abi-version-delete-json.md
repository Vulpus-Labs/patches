---
id: "0615"
title: Bump manifest ABI version; delete JSON audio-path code
priority: high
created: 2026-04-21
---

## Summary

New ABI is incompatible with old plugins. Bump
`FFI_ABI_VERSION` in `patches-ffi/src/scanner.rs` (or wherever
it lives). Delete the JSON runtime-update encode/decode path.
No compat shim — this subsystem has no external users.

## Acceptance criteria

- [ ] `FFI_ABI_VERSION` incremented.
- [ ] `json::serialize_parameter_map` deleted (or retained only
      for descriptor/manifest exchange — no runtime callers).
- [ ] Old-ABI scan rejects manifests cleanly with an informative
      error.
- [ ] `cargo clippy -p patches-ffi` clean.

## Notes

Epic E104.
