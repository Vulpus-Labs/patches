# E052 — Dynamic module loading via C ABI

## Goal

Enable runtime loading of modules from shared libraries (.dylib/.so/.dll),
allowing module authors to compile against `patches-core` and `patches-ffi`,
produce a `cdylib`, and have the host load it without recompilation.

After this epic:

- A new `patches-ffi` crate defines the C ABI contract (vtable, repr(C) types,
  JSON serialization for complex types, export macro, host-side loader).
- `patches-core` has minimal additions: `#[repr(C)]` on `CableValue`,
  `CablePool::as_raw_parts_mut()`, `Registry::register_builder()`.
- A plugin scanner discovers `.dylib`/`.so`/`.dll` files in a directory and
  registers them in the module registry.
- The ConvolutionReverb can be compiled as an external plugin and loaded at
  runtime, including its processing threads, file I/O, and `PeriodicUpdate`
  support.

## Background

ADR 0025 documents the design. Key decisions:

- **Hot path (process)**: raw CablePool pointer pass-through, zero overhead.
- **Control-thread types**: JSON serialization for `ModuleDescriptor` and
  `ParameterMap`.
- **Drop ordering**: `vtable.drop` joins plugin threads before `Arc<Library>`
  releases the library handle. Safe on the cleanup thread.
- **Trust model**: same as VST3/CLAP/AU/LV2 — in-process loading, OS-level
  protections, no host-side signature verification.

## Tickets

| ID   | Title                                      | Dependencies |
|------|--------------------------------------------|--------------|
| 0262 | patches-core: repr(C) and FFI accessors    | —            |
| 0263 | patches-ffi: crate scaffold and repr(C) types | 0262      |
| 0264 | patches-ffi: JSON serialization            | 0263         |
| 0265 | patches-ffi: host-side loader and DylibModule | 0264      |
| 0266 | patches-ffi: export_module! macro          | 0264         |
| 0267 | Test plugin: Gain cdylib                   | 0265, 0266   |
| 0268 | Test plugin: ConvolutionReverb cdylib      | 0267         |
| 0269 | Plugin scanner and registry integration    | 0265         |
