---
id: "0264"
title: "patches-ffi: JSON serialization for ModuleDescriptor and ParameterMap"
priority: high
created: 2026-04-07
---

## Summary

Implement JSON serialization and deserialization for `ModuleDescriptor` and
`ParameterMap` in `patches-ffi`. These types contain `Vec`, `HashMap`,
`&'static str`, and nested enums that cannot cross the C ABI directly. They
only cross the boundary on the control thread, so serialization overhead is
acceptable.

## Acceptance criteria

- [ ] `ModuleDescriptor` can be serialized to JSON bytes and deserialized back, round-tripping correctly
- [ ] `ParameterMap` can be serialized to JSON bytes and deserialized back, round-tripping correctly
- [ ] All `ParameterValue` variants are handled: Float, Int, Bool, Enum, String, Array
- [ ] All `ParameterKind` variants are handled: Float (with min/max/default), Int, Bool, Enum, String, Array
- [ ] `PortDescriptor` fields serialized: name, index, kind (Mono/Poly)
- [ ] Deserialized `&'static str` fields (module_name, port names, parameter names, enum variants) are produced by leaking `String`s via `Box::leak`
- [ ] `BuildError` messages can be serialized as UTF-8 bytes for error reporting across the boundary
- [ ] Unit tests for round-trip correctness of both types with representative data
- [ ] `cargo clippy` clean

## Notes

- The serializer is hand-rolled (no serde dependency on patches-core). If the
  implementation becomes unwieldy, serde can be introduced behind a feature
  flag in a follow-up.
- The `&'static str` leak is bounded: one set of leaked strings per module type
  per library load. This is documented and intentional.
- `ParameterKind::Enum { variants: &'static [&'static str] }` requires leaking
  both the individual strings and the slice itself.
- `ModuleShape` is embedded in `ModuleDescriptor` and must be included in the
  JSON representation.

Epic: E052
ADR: 0025
Depends: 0263
