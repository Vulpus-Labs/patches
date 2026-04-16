---
id: "0492"
title: FfiPluginManifest type and ABI v2 bump
priority: high
created: 2026-04-16
---

## Summary

Introduce `FfiPluginManifest` in `patches-ffi-common::types` and bump
`ABI_VERSION` from 1 to 2. The manifest carries an array of
`FfiPluginVTable`s so a single plugin can expose multiple module
types from one entry symbol. The vtable struct itself is unchanged.

## Acceptance criteria

- [ ] `FfiPluginManifest { abi_version: u32, count: usize, vtables: *const FfiPluginVTable }` added to `patches-ffi-common/src/types.rs` with `#[repr(C)]`.
- [ ] `ABI_VERSION` bumped to `2`.
- [ ] Re-exports updated in `patches-ffi/src/types.rs` and `patches-ffi/src/lib.rs`.
- [ ] Unit test: round-trip a leaked `[FfiPluginVTable; 2]`, reconstruct as a slice, verify counts and pointers.
- [ ] `cargo build`, `cargo clippy` clean (loader/macro will fail to compile until 0493/0494 land — coordinate as one merge or stack the PRs).

## Notes

ADR 0039. The pointer in `vtables` addresses plugin-static storage
(e.g. a `Box::leak`'d `Vec` or a `&'static [FfiPluginVTable; N]`);
the host clones each entry into its own `DylibModuleBuilder` and
does not retain the pointer.
