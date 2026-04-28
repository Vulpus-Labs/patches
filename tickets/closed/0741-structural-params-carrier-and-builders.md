---
id: "0741"
title: Add StructuralParams carrier and structural-param builders (additive)
priority: high
created: 2026-04-28
epic: "E126"
parent: "0734"
adrs: ["0060"]
---

## Summary

First slice of 0734 (ADR 0060). Purely additive: introduce the
`StructuralParams` control-thread carrier type and add structural-param
builder methods to `ModuleDescriptor` that push into a new
`structural_params: Vec<ParameterDescriptor>` field. No existing call
sites change behaviour. After this lands, `realtime_params` is the
existing `parameters` field renamed; structural builders work but no
module declares any structural params yet.

## Acceptance criteria

- [ ] `StructuralParams` type added to `patches-core` (control-thread
      carrier, allocation OK, no audio-thread accessors). Stores
      name+index → `ParameterValue` (extended with `String` variant if
      not already present, gated to structural use).
- [ ] `ModuleDescriptor.parameters` renamed to `realtime_params`.
- [ ] `ModuleDescriptor.structural_params: Vec<ParameterDescriptor>`
      field added; defaults empty.
- [ ] Builders `structural_string_param`, `structural_bool_param`,
      `structural_int_param`, `structural_float_param` push into
      `structural_params`.
- [ ] All workspace call sites updated for the field rename. Empty
      `structural_params: vec![]` added to every `ModuleDescriptor`
      literal.
- [ ] `compute_layout` and `pack_into` continue to read only
      `realtime_params` (rename mechanical).
- [ ] `cargo test` and `cargo clippy` pass on the inner-loop subset
      and on the full workspace.

## Notes

No `Module` trait changes here. `Module::prepare` still infallible,
unchanged signature. Structural builders compile but are unused.
This ticket is intentionally a no-op semantically.

Sweep strategy: `parameters: vec!` → `realtime_params: vec!,
structural_params: vec![]` for literals; `\.parameters\b` →
`.realtime_params` for field access. Hand-verify the half-dozen
`update_parameters` method names are not touched (they end in
`_parameters`, not `.parameters`).
