---
id: "0301"
title: "patches-engine: planner resolves File parameters via FileProcessor"
priority: high
created: 2026-04-11
---

## Summary

Extend the planner's `build_patch` to scan parameter maps for
`ParameterValue::File` entries, call the registry's `process_file`, and
replace them with `ParameterValue::FloatBuffer(Arc<[f32]>)` before the
plan reaches the audio thread.

## Acceptance criteria

- [ ] In `build_patch`, for both `NodeDecision::Install` and `NodeDecision::Update` paths, iterate parameter maps and resolve all `ParameterValue::File` entries
- [ ] For each `File(path)`, call `registry.process_file(module_name, env, shape, param_name, path)`
- [ ] On success, replace the entry with `FloatBuffer(Arc::from(result))`
- [ ] On failure, return `BuildError::ModuleCreationError` with the error message — plan building fails
- [ ] No `ParameterValue::File` values remain in the `ExecutionPlan` after `build_patch` completes
- [ ] Modules that do not implement `FileProcessor` but have `File` parameters produce a `BuildError`
- [ ] `cargo test -p patches-engine` passes
- [ ] `cargo clippy -p patches-engine` clean

## Notes

This runs on the control thread, so file I/O and heavy computation (FFT)
are safe. The planner already has access to `env`, `registry`, and the
module name for each node — all required to call `process_file`.

For the `Install` path, file resolution happens before `registry.create()`
so the module's `build()` / `update_parameters()` receives `FloatBuffer`
values. For the `Update` path, resolution happens before the parameter diff
is added to `parameter_updates`, so `update_validated_parameters` on the
audio thread also receives `FloatBuffer`.

Epic: E056
ADR: 0028
Depends: 0299
