---
id: "0498"
title: Split patches-interpreter descriptor_bind.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-interpreter/src/descriptor_bind.rs` is 925 lines mixing
`BindError` / `BindErrorCode` definitions with module-resolution
walking and connection/port/cable/layout validation.

## Acceptance criteria

- [ ] Convert to `descriptor_bind/mod.rs` with submodules:
      `errors.rs` (BindError, BindErrorCode, Display/Error impls),
      `modules.rs` (module-type resolution, shape validation,
      parameter validation delegation),
      `connections.rs` (port-existence, cable/layout agreement,
      duplicate-input detection, orphan port-ref checks).
- [ ] `bind` entry point stays in `mod.rs`.
- [ ] `BoundPatch` and public error re-exports unchanged at crate
      root.
- [ ] `cargo build -p patches-interpreter`,
      `cargo test -p patches-interpreter`, `cargo clippy` clean.

## Notes

E086. No behaviour change; `BN####` wire codes unchanged.
