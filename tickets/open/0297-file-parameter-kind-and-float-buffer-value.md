---
id: "0297"
title: "patches-core: File parameter kind and FloatBuffer value"
priority: high
created: 2026-04-11
---

## Summary

Add `ParameterKind::File` and `ParameterValue::File` / `FloatBuffer`
variants to patches-core so that modules can declare file parameters and
receive pre-processed float data.

## Acceptance criteria

- [ ] `ParameterKind::File { extensions: &'static [&'static str] }` variant added to `ParameterKind`
- [ ] `ParameterKind::File` has a `default_value()` returning `ParameterValue::File(String::new())`
- [ ] `ParameterKind::File` has a `type_name()` returning `"file"`
- [ ] `ParameterValue::File(String)` variant added — carries a resolved absolute path
- [ ] `ParameterValue::FloatBuffer(Arc<[f32]>)` variant added — carries processed file data
- [ ] `validate_parameters` accepts `FloatBuffer` where `File` is expected (planner has already resolved)
- [ ] `validate_parameters` accepts `File` where `File` is expected (pre-resolution)
- [ ] `ModuleDescriptor` builder gains `file_param(name, extensions)` and `file_param_multi(name, index, extensions)` methods
- [ ] Existing `ParameterValue` match sites updated (display, debug, clone, eq)
- [ ] `cargo test -p patches-core` passes
- [ ] `cargo clippy -p patches-core` clean

## Notes

`Arc<[f32]>` is chosen for `FloatBuffer` because `ParameterMap::clone()`
is called in the default `Module::update_parameters` impl. `Arc` makes
this O(1). Modules use `take_scalar` on the audio thread to extract the
`Arc` without cloning.

Epic: E056
ADR: 0028
