---
id: "0307"
title: "patches-ffi: File and FloatBuffer ABI support"
priority: medium
created: 2026-04-11
---

## Summary

Extend the FFI ABI to support `ParameterValue::File` and
`ParameterValue::FloatBuffer` across the native plugin boundary. This
includes JSON serialization for `File` values, a binary transfer path for
`FloatBuffer` data, and an optional `process_file` vtable entry for
external plugins that implement `FileProcessor`.

## Acceptance criteria

- [ ] `patches-ffi-common/src/json.rs`: `write_parameter_value` handles `File(path)` as `{"type":"file","v":"path"}`
- [ ] `patches-ffi-common/src/json.rs`: `deserialize_parameter_value` handles `"file"` type → `ParameterValue::File`
- [ ] `FloatBuffer(Arc<[f32]>)` is serialized as a binary payload, not JSON — the vtable uses a separate `FfiBytes`-based mechanism to pass float data to `update_validated_parameters` without JSON round-tripping megabytes of floats
- [ ] `FfiPluginVTable` gains an optional `process_file` entry: `unsafe extern "C" fn(env: FfiAudioEnvironment, shape: FfiModuleShape, param_name: *const u8, param_name_len: usize, path: *const u8, path_len: usize, result_out: *mut FfiBytes) -> i32` (returns 0 on success, 1 on error with error message in `result_out`)
- [ ] A sentinel (null function pointer or `supports_file_processor` flag) indicates whether the plugin implements `FileProcessor`
- [ ] `DylibModuleBuilder` calls `vtable.process_file` when the host's planner resolves `File` parameters for external plugins
- [ ] Host-side: `DylibModuleBuilder` wraps the returned `FfiBytes` float data in `Arc<[f32]>` and frees the plugin-allocated bytes via `vtable.free_bytes`
- [ ] Plugin-side (`export_module!`): when the module implements `FileProcessor`, the macro generates the `process_file` export that delegates to `M::process_file`
- [ ] ABI_VERSION bumped (this is a vtable layout change)
- [ ] Round-trip test: host sends `File` param, plugin's `process_file` is called, `FloatBuffer` flows back
- [ ] `cargo clippy` clean for `patches-ffi`, `patches-ffi-common`

## Notes

The key design tension is `FloatBuffer` serialization. JSON-encoding a
100k-float IR as `[0.001, -0.023, ...]` is ~1MB of text. Instead, the
host should pass `FloatBuffer` data as raw `f32` bytes alongside the
JSON-encoded scalar parameters.

One approach: split `update_validated_parameters` into two phases — JSON
for scalar params, then a separate call for each `FloatBuffer` value
identified by parameter name. Alternatively, extend the JSON format with a
reference scheme (`{"type":"float_buffer","ref":0}`) and pass the actual
buffers out-of-band via a parallel `FfiBytes` array.

The WASM boundary (`patches-wasm`) will need a similar treatment but can
be addressed in a follow-up ticket since the WASM crates are currently
shelved.

Epic: E056
ADR: 0028
Depends: 0297, 0299
