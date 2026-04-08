---
id: "0263"
title: "patches-ffi: crate scaffold and repr(C) types"
priority: high
created: 2026-04-07
---

## Summary

Create the `patches-ffi` crate and define the `#[repr(C)]` mirror types and
the `FfiPluginVTable` struct that form the ABI contract between host and plugin.

## Acceptance criteria

- [ ] `patches-ffi` crate exists in the workspace root, added to `Cargo.toml` workspace members
- [ ] Depends on `patches-core` and `libloading`
- [ ] `#[repr(C)]` types defined with `From`/`Into` conversions to their patches-core counterparts:
  - `FfiAudioEnvironment` (sample_rate: f32, poly_voices: usize, periodic_update_interval: u32)
  - `FfiModuleShape` (channels: usize, length: usize, high_quality: u8)
  - `FfiInputPort` (tag: u8 for Mono/Poly, cable_idx: usize, scale: f32, connected: u8)
  - `FfiOutputPort` (tag: u8 for Mono/Poly, cable_idx: usize, connected: u8)
  - `FfiBytes` (ptr: *mut u8, len: usize) for owned byte buffers
- [ ] `FfiPluginVTable` struct defined with all function pointer fields (describe, prepare, update_parameters, update_validated_parameters, process, set_ports, periodic_update, descriptor, instance_id, drop, free_bytes) plus `abi_version: u32` and `supports_periodic: i32`
- [ ] `ABI_VERSION` constant defined
- [ ] `cargo test` and `cargo clippy` clean

## Notes

- `bool` fields become `u8` across the ABI (C has no standard bool size).
- `InputPort`/`OutputPort` are flattened tagged structs rather than Rust enums.
- The `process` function pointer takes `(*mut [CableValue; 2], usize, usize)` —
  it uses `CableValue` directly (now `repr(C)` from T-0262), not a mirror type.
- `FfiBytes` is the protocol for plugin-allocated buffers: the host reads the
  data, then calls `free_bytes` to let the plugin deallocate.

Epic: E052
ADR: 0025
Depends: 0262
