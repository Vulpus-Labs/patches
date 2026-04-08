---
id: "0265"
title: "patches-ffi: host-side loader and DylibModule"
priority: high
created: 2026-04-07
---

## Summary

Implement the host side of the FFI plugin system: `DylibModule` (a `Module`
impl that delegates to a loaded plugin via the vtable), `DylibModuleBuilder`
(a `ModuleBuilder` impl for the registry), and `load_plugin()`.

## Acceptance criteria

- [ ] `DylibModule` struct holds opaque handle, vtable, cached descriptor, instance_id, and `Arc<libloading::Library>`
- [ ] `DylibModule` implements `Module`:
  - `descriptor()` returns cached descriptor
  - `instance_id()` returns stored id
  - `process()` extracts raw parts from CablePool via `as_raw_parts_mut()`, calls `vtable.process` — zero allocation
  - `set_ports()` converts `InputPort`/`OutputPort` slices to `FfiInputPort`/`FfiOutputPort`, calls `vtable.set_ports`
  - `update_validated_parameters()` serializes ParameterMap to JSON, calls vtable
  - `update_parameters()` serializes ParameterMap, calls vtable, deserializes error on failure
  - `as_any()` returns self
  - `as_periodic()` returns `Some(self)` when `vtable.supports_periodic != 0`, where `DylibModule` implements `PeriodicUpdate` by calling `vtable.periodic_update`
  - `as_midi_receiver()` returns `None` (MIDI not supported for external plugins)
- [ ] `DylibModule::drop()` calls `vtable.drop(handle)` before `Arc<Library>` decrements
- [ ] `unsafe impl Send for DylibModule` with documented safety contract
- [ ] `DylibModuleBuilder` implements `ModuleBuilder`:
  - `describe()` calls `vtable.describe`, deserializes JSON, frees bytes via `vtable.free_bytes`
  - `build()` calls describe, prepare, constructs DylibModule, calls `update_parameters`
- [ ] `load_plugin(path) -> Result<DylibModuleBuilder, String>`:
  - Opens library via `libloading::Library::new`
  - Resolves `patches_plugin_init` symbol
  - Calls it to obtain vtable
  - Checks `abi_version` — returns error on mismatch
  - Returns `DylibModuleBuilder` with `Arc<Library>`
- [ ] `cargo clippy` clean

## Notes

- `DylibModule::process()` is the hot path. The only work is extracting three
  values from CablePool and an indirect call through the vtable function
  pointer. No serialization, no allocation.
- Drop ordering is critical: `vtable.drop` must complete (joining any plugin
  threads) before `Arc<Library>` is released. Rust's drop order (fields in
  declaration order) guarantees this as long as `handle` and `vtable` are
  declared before `_lib`.
- The `PeriodicUpdate` impl on `DylibModule` passes a read-only CablePool
  pointer via `as_raw_parts` (needs a `&self` variant of the accessor, or
  pass `pool_ptr as *const`).

Epic: E052
ADR: 0025
Depends: 0264
