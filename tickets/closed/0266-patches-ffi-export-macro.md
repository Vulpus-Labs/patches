---
id: "0266"
title: "patches-ffi: export_module! macro"
priority: high
created: 2026-04-07
---

## Summary

Implement the `export_module!` macro that plugin authors use to generate the
`extern "C"` entry point and all vtable wrapper functions for a `Module` impl.

## Acceptance criteria

- [ ] `export_module!(MyModule)` generates:
  - `#[no_mangle] pub extern "C" fn patches_plugin_init() -> FfiPluginVTable`
  - `extern "C"` wrapper functions for each vtable entry
- [ ] Wrapper for `describe`: converts `FfiModuleShape` to `ModuleShape`, calls `T::describe`, serializes result to JSON, returns `FfiBytes`
- [ ] Wrapper for `prepare`: deserializes `ModuleDescriptor` from JSON, calls `T::prepare`, returns opaque `*mut c_void` (box-leaked pointer)
- [ ] Wrapper for `update_validated_parameters`: deserializes `ParameterMap` from JSON, calls method on module
- [ ] Wrapper for `update_parameters`: deserializes `ParameterMap`, calls method, serializes error into `FfiBytes` on failure, returns error code
- [ ] Wrapper for `process`: casts opaque pointer to `&mut T`, reconstructs `CablePool` from raw parts, calls `T::process` — no serialization
- [ ] Wrapper for `set_ports`: converts `FfiInputPort`/`FfiOutputPort` slices to `InputPort`/`OutputPort`, calls method
- [ ] Wrapper for `periodic_update`: reconstructs read-only `CablePool`, calls `PeriodicUpdate::periodic_update` if the module supports it, returns flag
- [ ] Wrapper for `descriptor`: calls method, serializes to JSON, returns `FfiBytes`
- [ ] Wrapper for `instance_id`: calls method, returns `u64`
- [ ] Wrapper for `drop`: casts opaque pointer to `Box<T>`, drops it (runs `T::Drop`)
- [ ] Wrapper for `free_bytes`: reclaims `Vec<u8>` from raw parts, drops it
- [ ] Every `extern "C"` wrapper is wrapped in `std::panic::catch_unwind`
- [ ] `supports_periodic` flag is set correctly based on whether the module type implements `PeriodicUpdate` (may require a trait-detection pattern or explicit macro argument)
- [ ] `cargo clippy` clean

## Notes

- The macro is a `macro_rules!` declarative macro. A proc-macro is not needed
  since the generated code is mechanical and does not inspect the module's
  fields or methods.
- `catch_unwind` requires the closure to be `UnwindSafe`. The opaque pointer
  cast is `UnwindSafe` because we only access it through exclusive references.
- For `process`, the `catch_unwind` wrapper on panic should write zeros (silence)
  and return, rather than leaving the output cables in an undefined state.
- Detecting whether `T: PeriodicUpdate` at macro expansion time is non-trivial
  in `macro_rules!`. Options: (a) require the user to pass a flag
  `export_module!(MyModule, periodic)`, (b) always generate the periodic
  wrapper and have it call `Module::as_periodic()` at runtime (one branch per
  periodic-update cycle, negligible cost), or (c) use a trait-based const-eval
  trick. Option (b) is simplest and recommended.

Epic: E052
ADR: 0025
Depends: 0264
