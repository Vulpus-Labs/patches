---
id: "0154"
title: Collapse ModuleDescriptor builder repetition with a macro
epic: E026
priority: low
created: 2026-03-20
---

## Summary

`ModuleDescriptor` in `patches-core/src/modules/module_descriptor.rs` has ~300 lines of nearly-identical builder methods: `mono_in`, `mono_in_multi`, `mono_out`, `mono_out_multi`, `poly_in`, `poly_in_multi`, `poly_out`, `poly_out_multi`, `float_param`, `float_param_multi`, `int_param`, `int_param_multi`. Each differs only in the port kind and which `Vec` it appends to. This is a maintenance hazard: adding a new port kind requires touching many places, and the repetition obscures the actual logic.

## Acceptance criteria

- [ ] A `macro_rules!` macro (internal to the crate) generates the family of `{kind}_{direction}` and `{kind}_{direction}_multi` builder methods from a compact declaration.
- [ ] Public API is unchanged (same method names and signatures).
- [ ] No new `unsafe` code introduced.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

The macro need not be `pub` — it only needs to be visible within `module_descriptor.rs`. If the macro turns out to be harder to read than the repetition it replaces (e.g. due to complex token munching), document the trade-off clearly in a comment and consider whether the win is worth it.
