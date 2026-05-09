---
id: "0828"
title: Fix patches-svg FlatModule param_block_span field
priority: high
created: 2026-05-06
---

## Summary

`patches-svg/src/lib.rs` fails to compile under `cargo clippy
--all-targets`: 7× E0063 "missing field `param_block_span` in initializer
of `patches_dsl::FlatModule`" at lib.rs:127, 135, 205, 281, 289, 349, 357.

The field was added to `FlatModule` in `patches-dsl` but the svg crate's
test-only `FlatModule` constructions were not updated. Likely only surfaces
in the `--all-targets` (test) build, which is why it has not blocked
ordinary `cargo build`.

Fix is mechanical: add `param_block_span: None` (or the appropriate
`Provenance`) to each construction. While here, consider a
`FlatModule::stub_for_test` helper in `patches-dsl` to avoid this churn
next time the struct grows.

## Acceptance criteria

- [ ] `cargo clippy -p patches-svg --all-targets` clean
- [ ] `cargo test -p patches-svg` passes
- [ ] (optional) test helper added in `patches-dsl` and svg sites use it
- [ ] `just commit -p patches-svg` clean

## Notes

Discovered while running readability lints workspace-wide on 2026-05-06.
Blocks `patches-svg` from participating in those lint runs.
