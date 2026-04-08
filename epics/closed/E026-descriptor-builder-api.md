---
id: "E026"
title: Descriptor builder API
created: 2026-03-18
tickets: ["0133", "0134"]
---

## Summary

Every `Module::describe()` implementation constructs a `ModuleDescriptor` using struct
literals with manually assigned indices. This is verbose, error-prone when ports are
reordered, and cannot correctly express shape-driven multi-port modules without a manual
loop. ADR 0017 specifies a builder API that eliminates the boilerplate while remaining
fully backwards-compatible with existing struct literal construction.

## Tickets

| ID   | Title                                                        | Priority | Depends on |
|------|--------------------------------------------------------------|----------|------------|
| 0133 | Add `ModuleDescriptor` builder methods to `patches-core`    | high     | —          |
| 0134 | Migrate `patches-modules` `describe()` to builder API       | medium   | 0133       |

## Definition of done

- `ModuleDescriptor::new`, `mono_in`, `mono_out`, `poly_in`, `poly_out`,
  `mono_in_multi`, `mono_out_multi`, `poly_in_multi`, `poly_out_multi`,
  `float_param`, `int_param`, `bool_param`, `enum_param`, `array_param`,
  `float_param_multi`, `int_param_multi`, `bool_param_multi`, `enum_param_multi`,
  and `sink` are implemented in `patches-core`.
- Single-port methods always produce `index: 0`. Multi-port methods produce indices
  `0..count`.
- All `describe()` implementations in `patches-modules` use the builder API.
- Existing struct literal construction in non-module code (tests, `module_descriptor.rs`
  doc tests, etc.) may be left unchanged.
- `cargo build`, `cargo test`, `cargo clippy` pass with no warnings across all crates.
- No `unwrap()` or `expect()` in library code.
