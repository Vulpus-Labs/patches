# ADR 0039 — Multi-module FFI plugin bundles

**Date:** 2026-04-16
**Status:** Proposed

---

## Context

ADR 0025 established the FFI plugin format: a `.dylib`/`.so`/`.dll` exports
a single `patches_plugin_init() -> FfiPluginVTable` symbol, and the host
loader registers it as one module type. The `export_module!(T)` macro
generates that symbol for a single `Module` impl.

This works for one plugin = one module, but it forces a static-link copy
of every shared dependency into each plugin. For tightly-coupled module
families this is wasteful:

- The eight drum modules (`Kick`, `Snare`, `ClapDrum`, `ClosedHiHat`,
  `OpenHiHat`, `Tom`, `Claves`, `Cymbal`) all depend on the same
  `patches-dsp` primitives — `DecayEnvelope`, `PitchSweep`,
  `MetallicTone`, `BurstGenerator`, `SvfKernel`, `MonoPhaseAccumulator`,
  `xorshift64`. Shipping them as eight separate `.dylib`s would link
  eight copies of the same DSP code.
- Independently versioned per-module artefacts also invite skew: a
  user could end up with a `Kick.dylib` built against one revision of
  `MetallicTone` and a `Cymbal.dylib` built against another.

Industry equivalent: VST3/CLAP/AU plugins routinely ship a single
`.vst3` containing many factory entries (Roland Cloud, NI Komplete,
etc.). A plugin author chooses bundling granularity; the host treats
each entry as an independent module.

### Constraints

1. **Source compatibility for single-module plugins.** Existing
   `export_module!(T)` callers (currently `test-plugins/gain`,
   `test-plugins/conv-reverb`) must keep working without source change.

2. **Zero hot-path overhead.** A bundle exposes N vtables; once the
   host has resolved a specific module's vtable at registration time,
   `process()` calls are exactly as cheap as today (one indirect call
   per cable-pool tick).

3. **One library handle per `.dylib`.** All `DylibModule` instances
   built from any vtable in a bundle share the same `Arc<Library>`.
   The library is unloaded only after every instance and every
   `DylibModuleBuilder` referencing it is dropped (ADR 0025 lifetime
   contract preserved).

4. **One entry symbol.** The loader looks for one well-known symbol;
   it does not enumerate the dylib's symbol table.

---

## Decision

### Manifest-returning entry symbol

Bump the ABI to version 2. The single entry symbol now returns a
manifest describing one or more vtables:

```rust
#[repr(C)]
pub struct FfiPluginManifest {
    pub abi_version: u32,
    pub count: usize,
    pub vtables: *const FfiPluginVTable, // array of length `count`
}

#[no_mangle]
pub extern "C" fn patches_plugin_init() -> FfiPluginManifest;
```

The `vtables` pointer addresses an array of `FfiPluginVTable`s held in
plugin-static storage (e.g. a `Box::leak`'d `Vec` or a `&'static
[FfiPluginVTable; N]`). The host reads the array at load time and
clones the vtable values into per-module `DylibModuleBuilder`s; it
does not retain the pointer.

`FfiPluginVTable` itself is unchanged. ABI v1 plugins (returning a
single vtable directly from `patches_plugin_init`) are rejected with
a clear error directing the author to recompile against the current
`patches-ffi`.

### `export_modules!` macro

A new macro registers any number of module types in one bundle:

```rust
patches_ffi::export_modules!(Kick, Snare, ClapDrum, ClosedHiHat,
                             OpenHiHat, Tom, Claves, Cymbal);
```

It expands to:

- One set of `extern "C"` wrapper functions per module type (the
  existing `__patches_ffi_*::<T>` family).
- A `static MANIFEST_VTABLES: [FfiPluginVTable; N]` populated at
  compile time.
- `patches_plugin_init` returning `FfiPluginManifest` pointing at
  `MANIFEST_VTABLES`.

`export_module!(T)` becomes a thin shim: `export_modules!(T)`. Existing
single-module plugins recompile without source change.

### Loader and scanner

`load_plugin(path) -> Result<Vec<DylibModuleBuilder>, String>` returns
a vec of builders, one per vtable in the manifest. All builders in the
result share an `Arc<libloading::Library>` for that path (constructed
once, cloned per builder).

`scan_plugins(dir)` flattens: each `(name, builder)` pair from each
plugin file appears separately in the returned vec. `register_plugins`
registers each in turn.

### Module-name uniqueness

Each vtable's `describe()` must yield a distinct `module_name` within
the bundle. Duplicate names within one manifest are a load-time error
(reported per-bundle, not per-module). Cross-bundle name collisions
follow the existing `Registry::register_builder` policy (last
registration wins, with a warning logged — unchanged).

### Drop order and library lifetime (unchanged)

`DylibModule` declares `handle` and `vtable` before `_lib` so that
`vtable.drop(handle)` runs before the `Arc<Library>` decrement (ADR
0025). With multi-module plugins the same rule holds for every
instance regardless of which vtable it came from; the `Arc<Library>`
is unloaded only after the last `DylibModule` and the last
`DylibModuleBuilder` for that path are dropped.

---

## Alternatives considered

### Add a second entry symbol, keep v1 working

Add `patches_plugin_init_v2` returning a manifest; loader tries v2
first, falls back to v1.

Rejected: only three internal plugins exist (`gain`, `conv-reverb`,
`gain-wasm`). Maintaining two ABIs forever to avoid recompiling three
plugins is not worth the complexity. Bumping `ABI_VERSION` and
recompiling is the cleaner choice.

### Per-module dylibs with a shared `patches-dsp` cdylib

Keep one dylib per module; ship `patches-dsp` itself as a cdylib that
each plugin links dynamically.

Rejected: makes `patches-dsp` part of the public ABI (every internal
struct layout becomes a compatibility constraint), and creates a
deployment dependency users must manage. Bundle linking sidesteps
both concerns.

### Symbol-table enumeration

Have the loader enumerate exported symbols matching a prefix
(`patches_module_*`) instead of relying on a manifest function.

Rejected: platform-specific (`dlsym` over the symbol table is awkward
on Windows), brittle against linker stripping, and harder to extend
with per-module metadata.

---

## Consequences

- **One ABI bump.** `ABI_VERSION` goes from 1 to 2; `gain`,
  `conv-reverb`, and `gain-wasm` test plugins recompile against the
  new `patches-ffi`. Their source is unchanged (`export_module!` shim).
- **Bundle plugins become possible.** A drum bundle (E088) ships
  eight modules in one `.dylib` with one copy of `patches-dsp`.
- **`patches-ffi` gains one type and one macro.** `FfiPluginManifest`
  in `patches-ffi-common::types`; `export_modules!` in
  `patches-ffi::export`.
- **Loader API breaks.** `load_plugin` now returns
  `Vec<DylibModuleBuilder>`; callers in `patches-player` and any tests
  update accordingly.
- **No hot-path change.** `process()` cost is identical; vtable
  resolution moves from one-per-file to N-per-file at registration
  time only.
