# ADR 0028 — File parameter type with planner-side processing

**Date:** 2026-04-11
**Status:** accepted

---

## Context

Modules that load external files (currently only `ConvolutionReverb` and
`StereoConvReverb`, which load impulse response audio files) face three
problems:

1. **Path resolution.** The DSL specifies file paths as plain strings. When
   running as a CLAP plugin, the process working directory is set by the DAW
   host and is unpredictable. Relative paths silently resolve to the wrong
   location or fail.

2. **Error reporting.** File loading currently happens asynchronously on a
   background thread (`IrLoader`). Errors are logged to stderr and silently
   swallowed — the module falls back to passthrough. The user has no
   indication that their IR file failed to load.

3. **Duplicated infrastructure.** Each file-loading module must implement its
   own background loading, teardown, and error handling. ConvolutionReverb
   has ~200 lines of `IrLoader` machinery. A second file-loading module
   would duplicate this pattern.

### Existing workaround

ConvolutionReverb declares a `String` parameter named `"path"`. The
interpreter passes it through as-is. On the audio thread,
`update_validated_parameters` stashes the path in a load request. A
dedicated `IrLoader` thread reads the file, builds the convolution
processor, and sends the result back via a ring buffer. The audio thread
polls for completion in `periodic_update`.

This works but places file I/O outside the normal error-reporting path and
forces each module to manage its own background loading lifecycle.

---

## Decision

### New DSL syntax: `file("path")`

A new value form `file("relative/or/absolute/path")` in the DSL denotes a
file reference. The parser produces a distinct AST node; the interpreter
creates a `ParameterValue::File(String)` carrying the resolved absolute
path (resolved against the patch file's parent directory, using the
base-dir mechanism introduced in the interpreter).

### New parameter kind and value variants

```rust
// In ParameterKind (module_descriptor.rs)
File { extensions: &'static [&'static str] }

// In ParameterValue (parameter_map.rs)
File(String)                  // Set by interpreter: resolved absolute path
FloatBuffer(Arc<[f32]>)       // Set by planner: processed file data
```

`ParameterKind::File` declares that a parameter expects a file. The
`extensions` slice lists accepted file extensions (e.g. `&["wav", "aiff"]`)
for validation and future LSP completion support.

`ParameterValue::File` carries the resolved path from the interpreter.
`ParameterValue::FloatBuffer` carries the processed result. The planner
transforms `File` → `FloatBuffer` before the parameter reaches any module.

`Arc<[f32]>` is chosen over `Vec<f32>` because `ParameterMap::clone()` is
called in the default `Module::update_parameters` impl on the control
thread. `Arc` makes this O(1) instead of copying potentially large buffers.

### FileProcessor trait

```rust
pub trait FileProcessor {
    fn process_file(
        env: &AudioEnvironment,
        shape: &ModuleShape,
        param_name: &str,
        path: &str,
    ) -> Result<Vec<f32>, String>
    where
        Self: Sized;
}
```

A module that accepts `File` parameters implements `FileProcessor`. The
method is static (no `&self`) — it runs on the control thread during plan
building, before any module instance is involved. It receives the audio
environment (for sample rate), the module shape (for quality settings), the
parameter name (to distinguish multiple file parameters), and the resolved
absolute path.

The return value is `Vec<f32>`. Its interpretation is private to the module
— it may contain raw samples, FFT'd spectral data, or any other
float-encoded representation. The planner wraps the result in
`Arc<[f32]>` and stores it as `ParameterValue::FloatBuffer`.

### Planner integration

The planner (in `build_patch`) processes file parameters for both
`NodeDecision::Install` and `NodeDecision::Update` paths:

1. Iterate parameters in the `ParameterMap`.
2. For each `ParameterValue::File(path)`, look up `FileProcessor` support
   in the registry and call `process_file`.
3. Replace `File(path)` with `FloatBuffer(Arc::from(data))`.
4. On failure, return `BuildError` — the error propagates to the CLAP GUI
   status line or the player console.

This runs on the control thread. By the time parameters reach the audio
thread (via `update_validated_parameters`), all file values have been
replaced with pre-processed data.

### ConvolutionReverb: pre-computed spectral data

For ConvolutionReverb, `process_file` reads the audio file, resamples to
the target sample rate, and performs the full FFT partitioning
(`NonUniformConvolver` tier structure). The returned `Vec<f32>` contains
the frequency-domain partition data in a layout private to the module.

On the audio thread, the module receives `FloatBuffer(Arc<[f32]>)` and
reconstructs a `NonUniformConvolver` from the pre-FFT'd data — skipping
the expensive forward FFT step. Only runtime infrastructure (processor
thread, ring buffers, overlap buffer pool) needs to be built, which
involves small, bounded allocations on the background thread.

This eliminates the `IrLoader` thread and its teardown machinery from
ConvolutionReverb, replacing ~200 lines of async lifecycle management with
a single `process_file` implementation.

### Registry changes

The `ModuleBuilder` trait (or a parallel registration mechanism) must
expose `process_file` to the planner without requiring knowledge of
concrete module types. The registry stores an optional function pointer
alongside each module builder, populated when the module implements
`FileProcessor`.

### FFI ABI

The existing FFI ABI serializes `ParameterMap` as JSON for
`update_validated_parameters` and `update_parameters`. Two new concerns
arise: calling `process_file` across the plugin boundary, and transmitting
`FloatBuffer` data to the plugin.

**`process_file` vtable entry.** An optional function pointer is added to
`FfiPluginVTable`:

```c
// Returns 0 on success, 1 on error (error message in result_out).
int (*process_file)(
    FfiAudioEnvironment env,
    FfiModuleShape shape,
    const uint8_t *param_name, size_t param_name_len,
    const uint8_t *path, size_t path_len,
    FfiBytes *result_out
);
```

A null pointer indicates the plugin does not implement `FileProcessor`.
On success, `result_out` contains raw `f32` bytes (little-endian,
platform-native) allocated by the plugin; the host wraps them in
`Arc<[f32]>` and frees the plugin allocation via `vtable.free_bytes`.

**`FloatBuffer` is not JSON-serialized.** Encoding megabytes of floats as
decimal JSON text is prohibitively wasteful. Instead, `FloatBuffer` values
are excluded from the JSON payload and transmitted as a binary sideband.

The mechanism: when the host serializes a `ParameterMap` containing
`FloatBuffer` values for an FFI plugin, it replaces each `FloatBuffer`
with a JSON placeholder `{"type":"float_buffer","ref":N}` where `N` is an
index into a parallel array of `(pointer, length)` pairs. The vtable call
is extended to accept this sideband:

```c
void (*update_validated_parameters)(
    void *handle,
    const uint8_t *params_json, size_t params_json_len,
    const FfiFloatBuffer *buffers, size_t buffers_len
);
```

where `FfiFloatBuffer` is:

```c
typedef struct {
    const float *ptr;
    size_t len;  // number of f32 elements
} FfiFloatBuffer;
```

The plugin-side deserializer resolves `{"type":"float_buffer","ref":N}` by
indexing into the `buffers` array. The data is a read-only borrow for the
duration of the call; the plugin must copy any data it wishes to retain.

This is a **vtable layout change** and requires an `ABI_VERSION` bump.
Existing plugins compiled against the old ABI will fail the version check
at load time — this is the intended behaviour.

---

## Alternatives considered

### Add base_dir to AudioEnvironment

Thread the patch file's directory through `AudioEnvironment` so modules can
resolve paths themselves.

Rejected: `AudioEnvironment` is `Copy` and used in ~50 construction sites
including `const` contexts. Adding `PathBuf` would break `Copy`, require a
large mechanical refactoring, and cross the FFI boundary
(`FfiAudioEnvironment` is `repr(C)`). More importantly, this leaves
file loading inside the module, perpetuating the async-loading and
silent-error patterns.

### Return `Box<dyn Any>` from process_file

Allow modules to return arbitrary types from file processing.

Rejected: `Box<dyn Any>` cannot be stored in `ParameterValue` without
making the enum trait-object-aware. `Vec<f32>` / `Arc<[f32]>` is sufficient — the
module controls both serialization (in `process_file`) and deserialization
(in `update_validated_parameters`), so the float layout is a private
contract.

### JSON-encode FloatBuffer data

Serialize `FloatBuffer` as a JSON array of floats, keeping the existing
vtable signatures unchanged.

Rejected: a 1-second stereo IR at 48kHz is 96,000 floats. JSON-encoded
with 6 decimal digits per float, this is ~1MB of text that must be
generated, transmitted, parsed, and converted back to `f32` — all on the
control thread during plan building. The binary sideband avoids this
entirely: the host passes a read-only pointer to the same `f32` data it
already holds.

### Process files in the interpreter

Have the interpreter call `process_file` when converting parameters.

Rejected: the interpreter does not have access to the registry's
`FileProcessor` implementations, and adding that dependency would violate
the interpreter's role as a validation-and-graph-building layer. The
planner is the right place — it already creates modules via the registry
and runs on the control thread.

---

## Consequences

- **File errors surface through the normal build path.** A missing IR file
  fails plan building with a clear error message, rather than silently
  falling back to passthrough.
- **Path resolution is centralised.** The interpreter resolves relative
  paths once; modules never see raw relative paths.
- **Module implementations simplify.** File-loading modules no longer need
  background loading threads, teardown channels, or async polling. The
  `process_file` method is a pure function from (env, shape, path) to data.
- **New ParameterValue variants.** `File` and `FloatBuffer` must be handled
  in validation, serialization (FFI JSON), and any code that pattern-matches
  on `ParameterValue`.
- **ABI version bump.** The `FfiPluginVTable` layout changes
  (`process_file` entry, extended `update_validated_parameters` signature,
  new `FfiFloatBuffer` type). All external plugins must be recompiled
  against the new ABI. This is enforced by the existing `ABI_VERSION` check.
- **Spectral pre-computation reduces first-note latency.** ConvolutionReverb
  no longer waits for an async FFT pipeline to complete before producing
  convolved output. The data is ready at plan adoption.
- **Larger plans.** `ExecutionPlan` may carry megabytes of float data for
  large IR files. This is acceptable — plan transfer is a pointer move
  through the ring buffer, not a copy.
