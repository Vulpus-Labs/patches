# ADR 0064 — Non-Rust FFI plugins (NAM as motivating case)

## Status

Deferred. Findings recorded for future revisit; no work scheduled.

## Context

Question raised: can the Neural Amp Modeller (NAM) C++ library be wrapped
as a `patches-ffi` plugin without writing any Rust? A successful
demonstration would prove that the plugin ABI is genuinely
language-neutral and not merely Rust-flavoured.

This ADR captures findings from a survey of the current FFI surface
against that goal. The work is not a priority right now; the intent is
to revisit when there is appetite for a third-party-language plugin
story.

The NAM model parameter would be **structural** (set at build, not
hot-swapped). Patch reload already triggers a full rebuild, so this
matches the existing structural-parameter mechanism (ADR 0060) and
removes the need for any control-thread model-swap plumbing.

## Findings

### What already works

The audio-thread vtable is genuinely C-clean:

- `FfiPluginVTable` is `#[repr(C)]`, all entry points `extern "C"`
  (`patches-ffi-common/src/types.rs`, `abi.rs`).
- `FfiPluginManifest` exposes a static array of vtables; load-time
  discovery is a single C function call.
- `ModuleDescriptor` crosses as JSON in `FfiBytes` from `describe` and
  back in to `prepare` as `descriptor_json`. JSON is trivial from any
  language.
- Panic-policy story (ADR 0051) is unwind-based on the Rust side; a
  C++ shim must catch exceptions at the FFI boundary itself, since
  foreign exceptions crossing FFI are UB regardless of unwind tables.

### Friction points

Three things make a non-Rust plugin author reverse-engineer from
source today:

1. **No published C header.** `cbindgen` is not wired up. Implementers
   must hand-translate `FfiPluginVTable`, `FfiBytes`, `FfiModuleShape`,
   `FfiAudioEnvironment`, `HostEnv`, and `CableValue`.
2. **`CableValue` is a `#[repr(C)]` Rust enum** — `Mono(f32) |
   Poly([f32; 16])`. Layout is stable but the C++ counterpart
   (`struct { uint32_t tag; union { float mono; float poly[16]; }; }`)
   has to be constructed by hand and kept in sync.
3. **Wire formats for parameter, port, and structural frames are
   defined only in Rust source.** `update_validated_parameters` and
   `set_ports` deliver positional packed bytes per ADR 0045 §5–6 and
   ADR 0060. The decoders live in `patches-ffi-common::param_frame`,
   `port_frame`, `structural_frame`. There is no spec document, no
   reference C decoder, and no published JSON schema for the
   `ModuleDescriptor` shape that drives layout. The `module_params!`
   macro that generates the layout is Rust-only.

A C++ plugin is mechanically buildable today; it just demands that the
implementer reads Rust source as authoritative documentation. That
undercuts the "language-neutral ABI" claim the demonstration is
supposed to make.

## Decision

Shelve the work. When revisited, treat it as a two-phase epic:

- **Phase 1 — ABI hardening for non-Rust plugins.** Standalone value
  (helps any future non-Rust plugin: Zig, Swift, C, etc.).
  - cbindgen-generated `patches_ffi.h` covering the vtable, manifest,
    `CableValue`, `FfiBytes`, `FfiModuleShape`, `FfiAudioEnvironment`,
    `HostEnv`.
  - Wire-format spec doc for ParamFrame, PortFrame, StructuralBlob —
    or a small C decoder helper shipped alongside the header.
  - Pinned JSON schema (or canonical example) for `ModuleDescriptor`.
  - Minimal C/C++ "hello plugin" smoke test under `test-plugins/`.

- **Phase 2 — NAM plugin.** The motivating demo.
  - Vendored NAM core built via CMake; static-linked Eigen.
  - C++ shim implementing the eight ABI entry points. NAM model path
    declared as a structural parameter.
  - Catch C++ exceptions at the FFI boundary; convert to error
    `FfiBytes` from `prepare`.
  - Example `.patches` file using the new module.

CPU cost is real — NAM models run 5–15 % of a single core at 48 kHz.
Document the per-instance cost in the module's manual page.

## Consequences

- No action now. The current Rust-only plugin surface is unchanged.
- When the work happens, Phase 1 must precede Phase 2; otherwise the
  NAM demo proves only "C++ can call Rust if you read enough Rust",
  not "the ABI is language-neutral".
- Phase 1 also creates the obligation to keep the published header /
  schema / wire spec in step with the Rust source. Today nothing
  outside the workspace consumes them, so changes are free; once a
  header ships, ABI changes need a bump and migration note.

## References

- ADR 0039 — multi-module plugin manifests
- ADR 0045 — frame-based audio-thread ABI
- ADR 0051 / E113 — panic policy and unwind requirements
- ADR 0060 — structural parameter flag
- `patches-ffi-common/src/types.rs`, `abi.rs`, `sdk.rs`
- `test-plugins/gain` — reference Rust plugin
