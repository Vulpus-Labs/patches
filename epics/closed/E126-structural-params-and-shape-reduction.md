---
id: "E126"
title: Structural params, ModuleShape reduction, and FileProcessor retirement
created: 2026-04-28
tickets: ["0734", "0741", "0742", "0743", "0744", "0735", "0736", "0737", "0745", "0738", "0739", "0740"]
adrs: ["0060"]
---

## Goal

Implement [ADR 0060](../../adr/0060-structural-parameter-flag.md):
introduce the structural-parameter tier, reduce `ModuleShape` to
`channels` only, retire the `FileProcessor` pipeline, and collapse the
DSL `shape_block` syntax. Brings construction-time params (including
file paths and quality flags) under one uniform mechanism, makes
file-backed modules FFI-able for the first time, and removes a stack
of incidental complexity (`File` / `FloatBuffer` `ParameterValue`
variants, `FloatBufferId` slots in `ParamFrame`, the `FileProcessor`
trait and registry, `resolve_file_params`).

## Scope

1. Split `ParameterDescriptor` into `realtime_params` and
   `structural_params`. Add `StructuralParams` carrier (free-form,
   control-thread, supports string-typed values).
2. Make `Module::prepare` fallible and absorb structural params as a
   constructor argument. Lift validate-and-pack out of
   `update_parameters` into a free function returning `ParamFrame`.
3. Reduce `ModuleShape` to `{ channels }`. Migrate `length` and
   `high_quality` to structural params on the modules that use them
   (`pitch_shift`, `delay`, `stereo_delay`).
4. Migrate `convolution_reverb` to declare `ir_path` as a structural
   string param. Retire the bespoke `update_parameters` override.
5. Delete `resolve_file_params`, the `FileProcessor` trait and
   registry, and the `File` / `FloatBuffer` `ParameterValue` variants.
   Remove `FloatBufferId` buffer slots from `ParamFrame` layout.
6. Collapse DSL `shape_block` grammar to a single positional arg
   (scalar or alias list). Delete `shape_arg` and the `channels:`
   named-key form. Migrate examples and update the LSP.
7. Extend the FFI: `prepare` entry point absorbs the structural blob
   (positional packed encoding mirroring `ParamFrame`). Add encode /
   decode helpers in `patches-ffi-common::sdk`. Update test-plugins.
8. Wire planner to detect structural-param edits and trigger
   instance-rebuild via the existing arc-table swap path.

## Acceptance

- ADR 0060 implemented end-to-end across core, modules, planner,
  DSL, FFI, and LSP.
- `cargo test` and `cargo clippy` pass on the inner-loop subset and
  full workspace.
- Existing example `.patches` files run unchanged in semantics
  (migration is mechanical; sound output identical).
- A test plugin demonstrates a file-backed module shipped over FFI
  (e.g. a minimal `ConvReverb` plugin proving the new ABI carries
  structural string params correctly).
- `FileProcessor`, `resolve_file_params`, `ParameterValue::File`,
  `ParameterValue::FloatBuffer`, and the `FloatBufferId` buffer slot
  type are removed from the codebase.
