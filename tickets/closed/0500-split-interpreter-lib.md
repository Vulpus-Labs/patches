---
id: "0500"
title: Split patches-interpreter lib.rs error types
priority: medium
created: 2026-04-16
---

## Summary

`patches-interpreter/src/lib.rs` is 819 lines. The crate entry
point also hosts `InterpretError`, `InterpretErrorCode`, the
unified error type from `build` / `build_with_base_dir`, and their
Display/Error impls.

## Acceptance criteria

- [ ] Pull error types into `error.rs`: `InterpretError`,
      `InterpretErrorCode`, unified error enum, all Display/Error
      impls.
- [ ] `lib.rs` retains `build`, `build_from_bound`,
      `build_with_base_dir`, registry/factory wiring, and
      re-exports for the moved error types so external callers
      still see them at crate root.
- [ ] `lib.rs` under ~500 lines.
- [ ] `cargo build -p patches-interpreter`,
      `cargo test -p patches-interpreter`, `cargo clippy` clean.

## Notes

E086. Public surface unchanged.
