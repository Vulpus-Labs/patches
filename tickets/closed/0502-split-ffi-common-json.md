---
id: "0502"
title: Split patches-ffi-common json.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-ffi-common/src/json.rs` is 751 lines covering the JSON
schema, serialisation helpers, and deserialisation helpers for the
FFI plugin boundary.

## Acceptance criteria

- [ ] Convert to `json/mod.rs` with submodules covering the
      serialise and deserialise halves (e.g. `ser.rs`, `de.rs`),
      plus `schema.rs` or similar for any shared schema
      definitions.
- [ ] Public API (functions/types used by `patches-ffi` and
      plugins) remains visible from the `json` module root via
      `pub use`.
- [ ] `cargo build -p patches-ffi-common`,
      `cargo test -p patches-ffi-common`, `cargo clippy` clean.

## Notes

E086. Confirm ser/de boundary on opening; adjust submodule names
if a different axis is more natural.
