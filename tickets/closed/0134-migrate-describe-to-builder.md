---
id: "0134"
title: Migrate patches-modules describe() implementations to builder API
priority: medium
created: 2026-03-18
---

## Summary

Replace the struct literal `ModuleDescriptor` construction in every `Module::describe()`
implementation in `patches-modules` with the builder API introduced in T-0133.

## Acceptance criteria

- [ ] Every `Module::describe()` implementation in `patches-modules` uses the builder
      API (`ModuleDescriptor::new(...).mono_in(...). ...`) rather than struct literals.
- [ ] No functional change: the resulting descriptors are identical to the originals
      (same module names, same port names, same indices, same parameter kinds and ranges).
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no warnings across all crates.

## Notes

Migration is mechanical. For each module:
1. Replace the `ModuleDescriptor { ... }` literal with `ModuleDescriptor::new(name, shape.clone())`.
2. Replace each `PortDescriptor { name, index: 0, kind: CableKind::Mono }` input with `.mono_in(name)`.
3. Replace multi-indexed port groups (if any) with the corresponding `_multi` call.
4. Replace each `ParameterDescriptor { name, index: 0, parameter_type: ParameterKind::Float { .. } }`
   with `.float_param(name, min, max, default)`, and similarly for other kinds.
5. Append `.sink()` if `is_sink: true`.

Struct literal construction elsewhere (tests, `module_descriptor.rs`) need not be changed.
