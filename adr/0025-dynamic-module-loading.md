# ADR 0025 — Dynamic Module Loading via C ABI

**Date:** 2026-04-07
**Status:** Proposed

---

## Context

All module implementations are currently compiled into the host binary via the
`patches-modules` crate. Adding a new module requires modifying the workspace,
rebuilding, and restarting the player. This prevents third-party module
development and limits the live-coding workflow to modules shipped with the
project.

The goal is runtime-loadable modules: a plugin author writes a Rust crate that
implements the `Module` trait, compiles it as a `cdylib`, and the host loads it
from a shared library at startup (or on hot-reload).

### Constraints

1. **No allocations on the audio thread.** The `process()` hot path must cross
   the ABI boundary with zero overhead — no serialization, no allocation, no
   indirection beyond the vtable function pointer call itself.

2. **Correct cleanup.** Modules may spawn threads (e.g. ConvolutionReverb runs
   FFT processing threads), perform I/O, and hold Arc-shared state. The shared
   library must not be unloaded while any plugin-spawned thread is still running.
   Drop must be orderly: plugin `Drop` runs first (joining threads), then the
   library handle is released.

3. **The engine's off-thread cleanup path must work.** Evicted modules are sent
   to a cleanup thread for deallocation (T-0052). A dynamically-loaded module
   dropped on the cleanup thread must behave identically to one dropped on the
   control thread.

4. **Minimal changes to patches-core.** The core crate should gain only what is
   strictly necessary to support FFI (a `repr(C)` annotation, an accessor, a
   registry method).

5. **Plugin author ergonomics.** A plugin author depends on `patches-core` and
   `patches-ffi`, implements `Module`, invokes a macro, and gets a loadable
   `.dylib`. No manual `extern "C"` functions.

### The ConvolutionReverb as reference case

The ConvolutionReverb is the most demanding module in the system and serves as
the validation target. It:

- Spawns 1-2 processing threads for FFT convolution.
- Performs file I/O (loading impulse response WAV files) during parameter
  updates on the control thread.
- Uses `Arc<SharedParams>` with `AtomicF32`/`AtomicBool` for lock-free
  communication between the audio thread and processing threads.
- Implements `PeriodicUpdate` for CV-driven parameter modulation.
- Joins all processing threads in its `Drop` impl.

Any design that cannot host ConvolutionReverb as an external plugin is
insufficient.

---

## Decision

### New crate: `patches-ffi`

A single new crate contains both the host-side loader and the plugin-side export
helpers. Both the host binary and plugin `.dylib` depend on it. This guarantees
the `#[repr(C)]` struct layouts and vtable signatures cannot drift between host
and plugin.

`patches-ffi` depends on `patches-core` and `libloading` (the only new external
dependency; a minimal, well-established crate for cross-platform shared library
loading).

### The plugin vtable

A plugin exports one C symbol:

```rust
#[no_mangle]
pub extern "C" fn patches_plugin_init() -> FfiPluginVTable
```

The `FfiPluginVTable` is a `#[repr(C)]` struct of function pointers mirroring
the `Module` trait, plus lifecycle management:

| Function                        | Thread      | Serialization | Notes                               |
|---------------------------------|-------------|---------------|-------------------------------------|
| `describe`                      | control     | JSON out      | Returns serialized ModuleDescriptor |
| `prepare`                       | control     | JSON in       | Creates opaque module instance      |
| `update_parameters`             | control     | JSON in       | Validates + applies; returns error  |
| `update_validated_parameters`   | control     | JSON in       | Applies pre-validated params        |
| `process`                       | **audio**   | **none**      | Raw CablePool pointer pass-through  |
| `set_ports`                     | audio       | repr(C)       | Infrequent; on topology change      |
| `periodic_update`               | audio       | **none**      | Raw CablePool pointer (read-only)   |
| `descriptor`                    | control     | JSON out      | From live instance                  |
| `instance_id`                   | control     | none          | Trivial                             |
| `drop`                          | cleanup     | none          | Frees instance, joins threads       |
| `free_bytes`                    | control     | none          | Frees plugin-allocated buffers      |

MIDI reception is not supported for dynamically-loaded plugins. MIDI routing
is handled by native modules; external plugins receive MIDI-derived signals
(e.g. V/Oct, gate) via ordinary cable connections.

The vtable carries `abi_version: u32` and a `supports_periodic` flag.

### How types cross the boundary

**Zero-cost (repr(C) or raw):**

- `CableValue` — add `#[repr(C)]` to the existing enum in patches-core. Both
  sides link the same patches-core, so the layout is identical. This is the
  single most important decision: it makes the `process()` hot path zero-cost.
- `CablePool` — passed as raw parts: `(*mut [CableValue; 2], usize, usize)`.
  The plugin reconstructs a `CablePool` from these parts (trivial struct init,
  no allocation). Requires a new `CablePool::as_raw_parts_mut()` accessor.
- `InputPort` / `OutputPort` — `#[repr(C)]` mirror structs in patches-ffi.
  Small, Copy, converted at `set_ports` time (infrequent).
- `AudioEnvironment`, `ModuleShape` — trivial `#[repr(C)]` mirrors.
- `InstanceId` — passed as raw `u64`.

**Serialized (JSON, control thread only):**

- `ModuleDescriptor` — serialized to/from JSON bytes. Contains `Vec`, `&'static
  str`, nested enums. Deserialized `&'static str` fields are produced by leaking
  `String`s (bounded, one per module type per library load).
- `ParameterMap` — serialized to/from JSON bytes for parameter updates.
- Error strings — UTF-8 bytes in `FfiBytes`.

The JSON serializer is hand-rolled in patches-ffi (avoids adding serde to
patches-core). The types are simple enough that this is a few hundred lines.

### Drop ordering and library lifetime

```
DylibModule {
    handle: *mut c_void,       // opaque pointer to plugin module instance
    vtable: FfiPluginVTable,   // function pointers into the library
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
    _lib: Arc<libloading::Library>,  // prevents library unload
}
```

Drop sequence:

1. `DylibModule::drop()` calls `(vtable.drop)(handle)`.
2. Inside the plugin, `Drop` for the concrete module runs — e.g.
   ConvolutionReverb signals shutdown, joins its processing threads.
3. `vtable.drop` returns. All plugin-spawned threads are now joined.
4. `Arc<Library>` ref count decrements. If this was the last instance, the
   library is unloaded.

This is safe on any thread (control thread, cleanup thread) because:

- `vtable.drop` is a blocking call that joins threads — this is expected on the
  cleanup thread (which exists for exactly this purpose).
- The engine guarantees modules are never dropped on the audio thread; they are
  sent to `cleanup_tx` (T-0052).
- Multiple `DylibModule` instances from the same plugin share an `Arc<Library>`
  via the `DylibModuleBuilder`. The library is unloaded only after all instances
  and the builder are dropped.

### Registry integration

`DylibModuleBuilder` implements the existing `ModuleBuilder` trait. A new method
`Registry::register_builder(name, builder)` accepts a `Box<dyn ModuleBuilder>`
directly (the existing `register::<T>()` is generic over concrete types). This
is a one-method addition to patches-core.

A loader function `load_plugin(path) -> Result<DylibModuleBuilder, String>`:

1. Opens the shared library via `libloading`.
2. Resolves the `patches_plugin_init` symbol.
3. Calls it to obtain the vtable.
4. Checks `abi_version` — rejects on mismatch.
5. Returns a `DylibModuleBuilder` holding the vtable and `Arc<Library>`.

### Plugin scanner

A `scan_plugins(dir) -> Vec<Result<(String, DylibModuleBuilder), String>>`
function in `patches-ffi` discovers and loads all plugins in a directory:

1. Enumerate files matching the platform-specific shared library extension
   (`.dylib` on macOS, `.so` on Linux, `.dll` on Windows).
2. For each file, call `load_plugin`. On success, call `describe` with a
   default shape to extract the module name.
3. Return a vec of `(module_name, builder)` pairs (or per-file errors).

A convenience function `register_plugins(dir, registry)` calls `scan_plugins`
and registers each successful builder via `Registry::register_builder`. Errors
are collected and returned (not fatal — a broken plugin does not prevent others
from loading).

The caller (e.g. `patches-player`) decides the plugin directory. A reasonable
default is a `plugins/` directory next to the patch file, or a
platform-specific user directory. The scanner is called during registry
construction, before the first patch is loaded.

### Plugin author experience

```rust
// my_reverb/src/lib.rs
use patches_core::*;
use patches_ffi::export_module;

pub struct MyReverb { /* ... */ }
impl Module for MyReverb { /* ... */ }
export_module!(MyReverb);
```

```toml
# my_reverb/Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
patches-core = { path = "../patches-core" }
patches-ffi = { path = "../patches-ffi" }
```

The `export_module!` macro generates:

- `patches_plugin_init()` returning a populated `FfiPluginVTable`.
- All `extern "C"` wrapper functions that convert types, delegate to the
  `Module` impl, and wrap calls in `catch_unwind`.

### Panic safety

Every `extern "C"` function generated by `export_module!` is wrapped in
`std::panic::catch_unwind`. Unwinding across a C ABI boundary is undefined
behaviour.

- Control-thread functions: on panic, return an error code + "plugin panicked"
  message via `FfiBytes`.
- `process()`: on panic, return silently (the module produces silence). A flag
  can be set for the host to log and optionally remove the module.

### Safety

- **`abi_version` check** rejects plugins compiled against a different
  patches-core version.
- **`catch_unwind`** at every FFI boundary prevents undefined behaviour from
  unwinding across the C ABI.
- **`unsafe impl Send for DylibModule`** — documented contract: the plugin's
  `Module` impl must be `Send`.
- **`FfiBytes` protocol** — plugin allocates, host reads, plugin frees via
  `free_bytes`; no double-free.
- **Drop safety** — `vtable.drop` joins plugin threads before library unload;
  `Arc<Library>` prevents premature unload.
- **Code execution trust** — loading a `.dylib` runs arbitrary code with the
  user's privileges, exactly as in VST3/CLAP/AU/LV2 hosts. OS-level
  protections (macOS Gatekeeper) apply; no additional host-side verification is
  performed, matching industry standard practice.

### Changes to patches-core

1. `cables.rs`: Add `#[repr(C)]` to `CableValue` (1 attribute, no behavioural
   change, all existing tests pass).
2. `cable_pool.rs`: Add `pub fn as_raw_parts_mut(&mut self) -> (*mut [CableValue; 2], usize, usize)`.
3. `registries/registry.rs`: Add `pub fn register_builder(&mut self, name: String, builder: Box<dyn ModuleBuilder>)`.

---

## Alternatives considered

### Function-pointer indirection for CablePool access

Instead of passing raw pointers, pass function pointers for `read_mono`,
`write_mono`, etc. into the plugin.

Rejected: adds per-cable-access overhead in the innermost loop. A module with
4 inputs and 2 outputs at 48kHz makes 288,000 indirect calls per second.

### Process isolation (separate process + shared memory)

Run plugins in a child process, communicate via shared-memory ring buffers.

Rejected: far more complex, adds latency, and is unnecessary for the
single-user live-coding use case. Could be revisited if untrusted third-party
plugins become a concern.

### Serde for serialization

Use `serde` + `serde_json` for `ModuleDescriptor` / `ParameterMap`.

Deferred: would require adding serde as a dependency to patches-core (or at
least as an optional feature). A hand-rolled JSON serializer in patches-ffi
avoids this. If the serialization code grows unwieldy, serde can be introduced
later behind a feature flag.

### Dual-crate split (host-ffi + plugin-ffi)

Separate the host loader from the plugin export helpers into two crates.

Rejected: both sides must agree on identical `#[repr(C)]` layouts and vtable
signatures. A single crate is the simplest way to guarantee this. The crate is
small enough that the unused half (host code in a plugin build, plugin code in
the host) is dead-code-eliminated by the linker.

---

## Consequences

- **Third-party modules become possible** without modifying the workspace.
- **`CableValue` gains a stable ABI** (`repr(C)`). This constrains future
  changes to its layout — adding a variant would be a breaking ABI change
  requiring an `abi_version` bump.
- **One new external dependency** (`libloading`) is added to the workspace,
  scoped to `patches-ffi` only.
- **Plugin version coupling**: host and plugin must be compiled against the same
  `patches-core` version (or at least the same `abi_version`). Mismatches are
  detected at load time, not at runtime.
- **`&'static str` leaks**: deserialized module descriptors leak a bounded
  number of strings. This is intentional and documented.
- **Trust boundary**: loading a shared library executes arbitrary native code
  with full process privileges. This is the same trust model as VST3, CLAP, AU,
  and LV2 — all major plugin formats use in-process `dlopen`/`LoadLibrary`
  with no host-side signature verification. On macOS, Gatekeeper enforces
  notarization/signing on dynamically loaded libraries; on Windows,
  `LoadLibrary` does not check signatures or Mark of the Web, matching the
  behaviour of mainstream DAW hosts. We do not add our own verification layer
  — doing so would exceed industry-standard practice without meaningfully
  improving security.
