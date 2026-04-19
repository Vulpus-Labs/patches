---
id: "E096"
title: Enum parameter values as variant indices (ADR 0045 Spike 0)
created: 2026-04-19
depends_on: []
tickets: ["0573", "0574", "0575", "0576"]
---

## Goal

Retire string-based enum parameter matching across all shipping
modules in favour of variant-index matching through typed Rust
enums generated at the descriptor. This is Spike 0 of ADR 0045 and
standalone: it delivers value independent of the rest of the data-
plane migration and shrinks the surface of every subsequent spike.

After this epic:

- `Module::update_validated_parameters` takes `&ParameterMap`, not
  `&mut ParameterMap`. With `String` gone, no variant on the
  update path requires destructive take. This aligns the in-process
  API with the read-only `ParamView` from Spike 5, two spikes
  ahead of schedule.

- `ParameterValue::Enum` carries `u32` (variant index), not
  `&'static str`. No allocation, no string interning, no
  descriptor-agnostic runtime match on names.
- Every shipping module consumes enum parameters through a typed
  enum (e.g. `OscFmType { Linear, Logarithmic }`) emitted by a
  `params_enum!` macro whose discriminants match the descriptor's
  `variants` slice order.
- DSL source-text enum names still resolve correctly (the
  interpreter maps `Scalar::Str` → variant index at param
  conversion time), and LSP completions still surface variant
  names from the descriptor.
- `ParameterKind::String` and `ParameterValue::String` are
  removed entirely. The audit found no shipping module using
  either, so the supporting infrastructure (interpreter arm,
  FFI JSON codec, LSP completions, descriptor builder helper)
  is dead weight and is excised in this epic rather than carried
  forward for removal later.

## Tickets

| ID   | Title                                                              | Priority | Depends on |
| ---- | ------------------------------------------------------------------ | -------- | ---------- |
| 0573 | params_enum! macro producing typed enums matched to descriptors    | high     | —          |
| 0574 | Migrate ParameterValue::Enum to u32 index + drop &mut on update    | high     | 0573       |
| 0575 | DSL / interpreter / LSP round-trip tests for enum resolution       | medium   | 0574       |
| 0576 | Remove ParameterKind::String and ParameterValue::String entirely   | medium   | 0574       |

## Affected surface

- `patches-core`: `ParameterValue`, `test_support::macros`.
- `patches-interpreter`: `tracker.rs` enum resolution.
- `patches-modules`: `oscillator`, `poly_osc`, `lfo`, `drive`,
  `tempo_sync`, `fdn_reverb`, `convolution_reverb` (mono + stereo),
  `master_sequencer`.
- `patches-vintage`: `vchorus`.
- `patches-ffi-common`: JSON ser/de for `ParameterValue::Enum`.
- `patches-ffi`: test plugins consuming enum params
  (`conv_reverb_plugin`).
- `patches-lsp`: completions (read-only; should need no change
  beyond verifying variant-name list access path).

## Definition of done

- `ParameterValue::Enum` carries `u32`. No `&'static str` remains
  in the audio-thread enum representation.
- All nine shipping modules use the `params_enum!` macro. No
  module matches enum parameters against string literals.
- DSL source-level enum names continue to work in all existing
  fixture tests. Interpreter maps source names to variant indices
  at param conversion; an unknown name is a clean
  `ParamConversionError::OutOfRange`.
- LSP completion tests still pass. Variant names are rendered from
  the descriptor.
- FFI JSON wire format may emit either variant name or index
  (decision in ticket 0574), but decoded values are `u32`.
- `cargo test` green across the workspace. `cargo clippy` clean.

## Out of scope

- File param / ArcTable / FloatBuffer work (Spike 2+).
- Any change to the FFI audio-thread ABI (Spike 7).
- Removal of `ParameterValue::String` from the runtime type
  (Spike 5).

## Notes

The audit (2026-04-19) found no shipping modules declaring
`ParameterKind::String`. The Spike 0 goal of "convert
String-used-as-selection to Enum" therefore has no concrete
targets, and the work collapses to the variant-index migration of
existing `Enum` params.

The `params_enum!` macro and the type-payload change are separated
into two tickets so the macro can be reviewed in isolation before
the atomic migration lands.
