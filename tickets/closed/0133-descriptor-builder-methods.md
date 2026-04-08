---
id: "0133"
title: Add ModuleDescriptor builder methods to patches-core
priority: high
created: 2026-03-18
---

## Summary

Add a builder API to `ModuleDescriptor` in `patches-core` as specified in ADR 0017.
This is a purely additive change — all existing struct literal construction remains valid.

## Acceptance criteria

- [ ] `ModuleDescriptor::new(name, shape)` constructs an empty descriptor.
- [ ] Single-port methods `mono_in`, `mono_out`, `poly_in`, `poly_out` each push one
      `PortDescriptor` with `index: 0` and the appropriate `CableKind`.
- [ ] Multi-port methods `mono_in_multi`, `mono_out_multi`, `poly_in_multi`,
      `poly_out_multi` each take a `count: u32` and push descriptors with indices
      `0..count`.
- [ ] Single-parameter methods `float_param`, `int_param`, `bool_param`, `enum_param`,
      `array_param` each push one `ParameterDescriptor` with `index: 0`.
- [ ] Multi-parameter methods `float_param_multi`, `int_param_multi`, `bool_param_multi`,
      `enum_param_multi` each take a `count: usize` and push descriptors with indices
      `0..count`.
- [ ] `sink()` sets `is_sink: true`.
- [ ] All methods consume and return `Self` (builder pattern).
- [ ] Unit tests in `patches-core` cover: single-port methods produce correct
      `PortDescriptor` fields; multi-port method with `count=3` produces three entries
      with indices 0, 1, 2; parameter methods produce correct `ParameterDescriptor`
      fields; `sink()` sets `is_sink`.
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no warnings.

## Notes

See ADR 0017 for the full specification and before/after examples.

The `ParameterSpec` struct already exists in `module_descriptor.rs` (`name`, `kind` fields)
but is unused. The builder parameter methods supersede it; `ParameterSpec` can be removed
or left in place.
