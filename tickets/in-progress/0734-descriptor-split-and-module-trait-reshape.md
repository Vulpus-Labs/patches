---
id: "0734"
title: Split ParameterDescriptor and reshape Module trait around structural params
priority: high
created: 2026-04-28
epic: "E126"
adrs: ["0060"]
---

## Summary

Foundation work for ADR 0060. Split `ModuleDescriptor` into
`realtime_params` and `structural_params`. Add a `StructuralParams`
carrier that supports string-typed values. Make `Module::prepare`
fallible and take structural params as a constructor argument. Lift
validate-and-pack out of the trait into a free function so
`Module::update_parameters` (currently a default trait method that
just calls another trait method) can be deleted.

## Acceptance criteria

- [ ] `ModuleDescriptor` carries two parameter tables: `realtime_params`
      (existing types, packable, numeric) and `structural_params`
      (new — supports `String` / `PathBuf` plus the realtime types).
- [ ] `StructuralParams` type added to `patches-core` (control-thread
      carrier; allocation OK; no audio-thread accessors).
- [ ] `Module::prepare` signature becomes
      `fn prepare(&AudioEnvironment, ModuleDescriptor, InstanceId, &StructuralParams) -> Result<Self, BuildError>`.
- [ ] `validate_and_pack(descriptor, &ParameterMap) -> Result<ParamFrame, BuildError>`
      added as a free function. `Module::update_parameters` default
      trait method removed; `Module::build` calls
      `validate_and_pack` then `update_validated_parameters`.
- [ ] `compute_layout` only sees `realtime_params`; the packer
      statically refuses non-packable types (compile-time-prevented
      where possible, runtime error otherwise).
- [ ] Builders `.structural_string_param(...)`, `.structural_bool_param(...)`,
      `.structural_int_param(...)`, `.structural_float_param(...)` added
      on `ModuleDescriptor`. Existing realtime builders unchanged.
- [ ] All existing modules compile against the new trait (no behaviour
      changes yet — empty `structural_params` table, no-op
      `apply_structural` paths).
- [ ] `cargo test` and `cargo clippy` pass on the inner-loop subset.

## Notes

This ticket is intentionally a no-op semantically: every existing
module declares zero structural params, every `prepare` call passes an
empty `StructuralParams`, every realtime path is unchanged. Subsequent
tickets migrate specific modules and retire dead code.

Pay attention to the test harness: `ModuleHarness::build_full` and
friends need a `StructuralParams` arg added (default to empty).
