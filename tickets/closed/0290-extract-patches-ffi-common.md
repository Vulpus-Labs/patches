---
id: "0290"
title: "Extract patches-ffi-common from patches-ffi"
priority: high
created: 2026-04-08
---

## Summary

Extract the hand-rolled JSON serializer and shared `#[repr(C)]` types from
`patches-ffi` into a new `patches-ffi-common` crate. This enables both
`patches-ffi` (native plugins) and `patches-wasm` (WASM plugins) to share the
serialization code without duplication.

## Acceptance criteria

- [ ] `patches-ffi-common` crate exists in the workspace, added to root `Cargo.toml` members
- [ ] Depends on `patches-core` only (no `libloading`)
- [ ] Contains `json.rs` moved from `patches-ffi/src/json.rs` (full module, including tests)
- [ ] Contains shared types moved from `patches-ffi/src/types.rs`:
  - `FfiAudioEnvironment`, `FfiModuleShape`, `FfiInputPort`, `FfiOutputPort`
  - `FfiBytes` and its `From`/`Into` conversions
  - `ABI_VERSION` constant
  - Port tag constants (`PORT_TAG_MONO`, `PORT_TAG_POLY`)
- [ ] `FfiPluginVTable` stays in `patches-ffi/src/types.rs` (it contains `extern "C"` function pointers specific to native loading)
- [ ] `patches-ffi` depends on `patches-ffi-common` and re-exports what it needs
- [ ] All `patches-ffi` imports updated — no dead code, no broken references
- [ ] All existing `patches-ffi` tests pass unchanged
- [ ] `cargo test` and `cargo clippy` clean across workspace

## Notes

- `patches-ffi-common` must compile for `wasm32-unknown-unknown` (it will be
  used by `patches-wasm-sdk`). Verify with
  `cargo check --target wasm32-unknown-unknown -p patches-ffi-common`.
- The JSON module has no platform-specific code, so this should work without
  changes.
- `patches-ffi` should re-export `patches_ffi_common::json` so existing callers
  (e.g. the export macro) continue to work via `patches_ffi::json`.

Epic: E055
ADR: 0027
