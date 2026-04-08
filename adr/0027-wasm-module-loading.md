# ADR 0027 — WASM Module Loading

**Date:** 2026-04-08
**Status:** Spike complete, shelved

---

## Context

ADR 0025 introduced dynamic module loading via C ABI shared libraries
(`patches-ffi`). This enables third-party modules compiled as native `.dylib`/
`.so`/`.dll` files. However, native plugins carry two limitations:

1. **No sandboxing.** A native plugin has full process privileges. A bug (or
   malicious code) can corrupt host memory, crash the process, or access the
   filesystem. This is the standard trust model for audio plugins (VST3, CLAP,
   AU, LV2), but it limits distribution of community-authored modules.

2. **Platform-specific binaries.** A plugin must be compiled separately for each
   OS and architecture. Distribution requires multi-platform CI, code signing,
   and platform-specific packaging.

WASM addresses both: a `.wasm` module runs in a sandboxed linear memory that
cannot access host memory, and the same binary runs on any platform.

### Constraints

1. **Per-sample audio performance.** `process()` is called at 48kHz per module.
   The WASM call overhead (~50–100ns per call via wasmtime's AOT compiler) must
   fit within the ~20µs per-sample budget. This is acceptable (<0.5% overhead).

2. **No allocations on the audio thread.** The cable staging area in WASM linear
   memory must be pre-allocated at plan-build time.

3. **CableValue layout compatibility.** `CableValue` is `repr(C)` containing
   only `f32` and `[f32; 16]` — no `usize` or pointer types. The layout is
   identical between the host (native) and WASM module (wasm32), so raw byte
   copying is correct.

4. **Reuse existing infrastructure.** The JSON serialization for
   `ModuleDescriptor` and `ParameterMap` (currently in `patches-ffi/src/json.rs`)
   should be shared, not duplicated.

### Multi-language plugin authoring

WASM is a compilation target for many languages. The export contract
(`patches_describe`, `patches_process`, etc.) is language-agnostic. Any language
that compiles to WASM without a garbage collector runtime is suitable for
real-time audio modules:

- **Rust** — first-class support via `patches-wasm-sdk`
- **C/C++** — mature WASM toolchain (clang, Emscripten)
- **Zig** — native WASM target, no runtime
- **AssemblyScript** — TypeScript-like syntax, compiles to WASM without a JS
  runtime

Languages with managed runtimes and garbage collectors (Go, Kotlin/JVM, Java)
are unsuitable: GC pauses violate the real-time audio constraint.

**Note on C/C++ specifically:** C and C++ libraries already have the best native
FFI story of any language. If the goal is to use an existing C++ DSP library
(FAUST, STK, DaisySP, etc.) in your own patches, the native C ABI plugin system
(ADR 0025 / `patches-ffi`) is strictly better: no runtime overhead, no
copy-in/copy-out, no new dependencies. WASM adds value for C/C++ only in the
distribution case — a module author compiles to `.wasm` once and shares a
sandboxed, cross-platform binary. Multi-language support is a consequence of
choosing WASM for sandboxing, not a primary motivation.

---

## Decision

### New crates

**`patches-ffi-common`** — extracted from `patches-ffi`. Contains the hand-rolled
JSON serializer (`json.rs`) and shared `#[repr(C)]` types (`FfiInputPort`,
`FfiOutputPort`, `FfiModuleShape`, `FfiAudioEnvironment`, `FfiBytes`). Both
`patches-ffi` and `patches-wasm` depend on it.

**`patches-wasm`** — host-side WASM loader. Depends on `patches-core`,
`patches-ffi-common`, and `wasmtime`. Contains:

- `WasmModuleBuilder` implementing `ModuleBuilder`
- `WasmModule` implementing `Module`
- `.wasm` file scanner and registry integration
- AOT compilation cache

**`patches-wasm-sdk`** — plugin-side authoring SDK targeting
`wasm32-unknown-unknown`. Depends on `patches-core` and `patches-ffi-common`.
Contains:

- `export_wasm_module!` macro (analogous to `export_module!`)
- WASM-side `CablePool` shim for the staging area

### Memory model: copy-in/copy-out with cable remapping

WASM modules cannot address host memory. The host copies cable data into and
out of the WASM module's linear memory for each `process()` call.

To minimise overhead, only the cable slots that a module actually uses (known
from `set_ports`) are copied. Cable indices are remapped to a compact 0-based
range in the WASM staging area.

**Staging area layout** in WASM linear memory:

```
[CableValue; 2] × N    (N = number of cables this module touches)
```

The host maintains a mapping: `staging_slot[i] ↔ host_cable_idx[i]`.

**Per-sample sequence:**

1. **Copy in:** For each mapped cable, memcpy `pool[host_idx]` → WASM staging
   slot (both ping-pong entries, `sizeof([CableValue; 2])` = ~136 bytes each).
2. **Call `patches_process`:** WASM function reads inputs from / writes outputs
   to the staging area using 0-based cable indices.
3. **Copy out:** For each output cable, memcpy the write-slot back from WASM
   staging → `pool[host_idx][wi]`.

**Overhead for a typical module** (2 inputs, 1 output = 3 cables):
- Copy in: 3 × 136 bytes = 408 bytes
- Copy out: 1 × 68 bytes = 68 bytes
- WASM call: ~50–100ns
- Total: ~476 bytes memcpy + call overhead per sample

At 48kHz this is ~23MB/s memcpy per module. Acceptable.

### WASM export contract

A WASM module exports these functions:

| Export                              | Purpose                              |
|-------------------------------------|--------------------------------------|
| `patches_describe`                  | Returns JSON ModuleDescriptor        |
| `patches_prepare`                   | Initialise module singleton          |
| `patches_process`                   | Per-sample audio processing          |
| `patches_set_ports`                 | Deliver remapped port objects        |
| `patches_update_validated_parameters` | Apply pre-validated parameters     |
| `patches_update_parameters`         | Validate + apply parameters          |
| `patches_periodic_update`           | Periodic coefficient update          |
| `patches_supports_periodic`         | Whether periodic_update is supported |
| `patches_alloc`                     | Allocate in WASM linear memory       |
| `patches_free`                      | Free in WASM linear memory           |

The module singleton is stored as a `static mut` in the WASM module. This is
safe because WASM execution is single-threaded.

### Instance isolation

Each `WasmModule` instance owns its own `wasmtime::Store` and
`wasmtime::Instance`. The compiled `wasmtime::Module` (AOT-compiled code) is
shared via `Arc` across all instances from the same `.wasm` file.

### AOT compilation caching

On first load, wasmtime compiles `.wasm` → native code and serializes the
result to a `.wasmcache` file. Subsequent loads deserialize directly, skipping
compilation. The cache is invalidated by wasmtime version changes (embedded in
the serialized format) and by `.wasm` file modification.

### Plugin author experience (Rust)

```rust
// my_filter/src/lib.rs
use patches_core::*;
use patches_wasm_sdk::export_wasm_module;

pub struct MyFilter { /* ... */ }
impl Module for MyFilter { /* ... */ }
export_wasm_module!(MyFilter);
```

```toml
# my_filter/Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
patches-core = { path = "../patches-core" }
patches-wasm-sdk = { path = "../patches-wasm-sdk" }
```

Build: `cargo build --target wasm32-unknown-unknown`

The same `Module` implementation compiles to either a native `cdylib` (with
`export_module!` from `patches-ffi`) or a WASM module (with
`export_wasm_module!` from `patches-wasm-sdk`).

---

## Alternatives considered

### Shared memory (WASM imported memory)

Map the host's CablePool into the WASM module's linear memory space.

Rejected: the WASM specification does not allow a module to access arbitrary
host memory. While wasmtime supports shared memories between modules, the host
CablePool is not a WASM memory object. The copy-in/copy-out approach is
simpler, correct, and has acceptable overhead.

### Full CablePool copy (no remapping)

Copy the entire CablePool into WASM memory each sample.

Rejected: a patch with 100 cables would copy 100 × 136 = 13,600 bytes per
module per sample, regardless of how many cables the module uses. Remapping to
only the used cables reduces this to a small, bounded copy.

### WASI instead of bare wasm32-unknown-unknown

Use the WASI target for filesystem access, stdio, etc.

Deferred: audio modules have no need for filesystem or I/O access during
`process()`. Parameter updates (e.g. loading an impulse response file) happen
on the control thread where the host can mediate. WASI can be added later if
needed, without changing the core architecture.

### Adding WASM support to patches-ffi

Put the WASM loader in the existing `patches-ffi` crate.

Rejected: `patches-ffi` depends on `libloading` and defines C ABI types
(function pointers, extern "C" wrappers). WASM loading uses a fundamentally
different runtime (`wasmtime`) with different boundary mechanisms. A separate
crate follows the workspace convention of focused responsibilities.

---

## Consequences

- **Sandboxed plugins become possible.** A WASM module cannot corrupt host
  memory, crash the process, or access the filesystem. This enables safe
  distribution and loading of community-authored modules.
- **Cross-platform plugin binaries.** A single `.wasm` file runs on macOS,
  Linux, and Windows on both x86 and ARM without recompilation.
- **Multi-language module authoring.** C/C++, Zig, AssemblyScript, and other
  WASM-targeting languages can be used to write modules, not just Rust.
- **New dependency: wasmtime.** A significant compile-time addition (~200
  crates). Scoped to `patches-wasm` only; does not affect `patches-core` or
  other crates.
- **Slightly higher per-sample overhead than native plugins.** The copy-in/
  copy-out + WASM call adds ~100–200ns per module per sample, compared to
  ~5–10ns for native function pointer calls. This is acceptable for the
  expected module counts.
- **JSON serialization is shared infrastructure.** Extracting `patches-ffi-common`
  makes the serialization code a dependency of two crates, increasing the cost
  of changing the JSON format. This is mitigated by the fact that the format is
  already stable (used in production by `patches-ffi`).

---

## Implementation spike results

An implementation spike was completed covering the full vertical: Rust SDK,
wasmtime host loader, AOT cache, scanner, and two example modules. The spike
confirmed that the architecture works end-to-end. The crates are excluded from
the workspace build (see below) and should be treated as experimental reference
code, not production-ready.

### What was built

| Crate / directory              | Role                                                           |
|--------------------------------|----------------------------------------------------------------|
| `patches-ffi-common/`          | Extracted shared JSON + repr(C) types                          |
| `patches-wasm-sdk/`            | Rust plugin SDK: `export_wasm_module!` macro                   |
| `patches-wasm/`                | Host loader: `WasmModuleBuilder`, `WasmModule`, scanner, cache |
| `test-plugins/gain-wasm/`      | Minimal test plugin (gain) with integration tests              |
| `examples-wasm/wavefolder-rs/` | Example Rust WASM module: wavefolder with drive param + CV     |
| `examples-wasm/mid-side-as/`   | Example AssemblyScript WASM module: stereo mid/side splitter   |

All Rust WASM modules compile with `cargo build --target wasm32-unknown-unknown`.
The AssemblyScript module compiles with `npx asc`. The host loader successfully
loads, describes, prepares, and processes both.

### How it works

**Rust module authoring** is identical to native plugin authoring: implement the
`Module` trait and invoke `export_wasm_module!(MyModule)`. The macro generates
all 10 WASM exports. The module singleton is a `static mut` (safe: WASM is
single-threaded). Cable I/O goes through `CablePool` exactly as in native
modules.

**Host-side loading** uses wasmtime. `WasmModuleBuilder` compiles (or loads
cached) `.wasm` files and implements `ModuleBuilder`. Each `WasmModule` instance
owns its own `Store` and `Instance`; the compiled `Module` is shared via `Arc`.
Cable data is copied in/out of a staging area in WASM linear memory each sample.
Port indices are remapped to a compact 0-based range so the WASM module sees
contiguous staging slots.

**Non-Rust modules** (demonstrated with AssemblyScript) implement the same 10
exports directly. The current AssemblyScript example hardcodes the `CableValue`
memory layout and port wire format, which is fragile — see the SDK design below.

### AssemblyScript SDK design (not yet implemented)

The spike revealed that non-Rust modules must currently know the exact binary
layout of `CableValue` (68-byte `repr(C)` enum: 4-byte discriminant + 64-byte
payload), `WasmInputPort` (16 bytes), and `WasmOutputPort` (12 bytes). This
leaks ABI detail across the module boundary.

The recommended solution is a guest-side AssemblyScript SDK library
(`patches-as-sdk/`) — no host changes needed. The SDK would provide:

**Port types** — `MonoInput`, `MonoOutput`, `PolyInput`, `PolyOutput` classes
with `fromRaw(ptr, index)` factories that parse the wire format.

**CableIO** — wraps `cablePtr` + `writeIndex`, provides `@inline` methods
`readMono(input)`, `writeMono(output, value)`, `readPoly(input, buf)`,
`writePoly(output, buf)`. These inline to raw `load<f32>`/`store<f32>` — zero
per-sample overhead.

**DescriptorBuilder** — fluent API mirroring the Rust `ModuleDescriptor`:

```typescript
new DescriptorBuilder("Wavefolder", channels, length, hq)
  .monoIn("in").monoIn("drive_cv").monoOut("out")
  .floatParam("drive", 1.0, 20.0, 1.0)
```

Plus `intParam`, `boolParam`, `enumParam`, `stringParam`, `arrayParam`, and
`_multi` variants. Serialises to JSON matching the host's expected format.

**ParameterMap** — parsed from the JSON the host sends in
`patches_update_parameters`. Typed accessors: `getFloat(name)`, `getInt(name)`,
`getBool(name)`, `getEnum(name)`, `getString(name)`, `getArray(name)`, each
with an optional `index` for multi-indexed parameters.

**Abstract PatchesModule class** — module authors extend this:

```typescript
abstract class PatchesModule {
  abstract describe(channels: i32, length: i32, hq: bool): DescriptorBuilder;
  abstract prepare(env: AudioEnv): void;
  abstract updateParameters(params: ParameterMap): void;
  abstract setPorts(inputs: InputPorts, outputs: OutputPorts): void;
  abstract process(cables: CableIO): void;
  // Optional: periodicUpdate(cables: CableIO): bool
  // Optional: supportsPeriodic(): bool
}
```

Where `InputPorts` / `OutputPorts` wrap the raw port pointer and provide
`mono(index)` / `poly(index)` accessors.

**Export glue** — `registerModule(new MyModule())` + `export * from
"patches-as-sdk/assembly/glue"` re-exports all `patches_*` functions. Total
boilerplate: two lines.

**Example module using the SDK** (mid/side splitter):

```typescript
import { PatchesModule, DescriptorBuilder, ParameterMap,
         MonoInput, MonoOutput, CableIO, AudioEnv,
         InputPorts, OutputPorts, registerModule } from "patches-as-sdk";
export * from "patches-as-sdk/assembly/glue";

class MidSide extends PatchesModule {
  inLeft: MonoInput = new MonoInput();
  inRight: MonoInput = new MonoInput();
  // ... other ports ...

  describe(channels: i32, length: i32, hq: bool): DescriptorBuilder {
    return new DescriptorBuilder("MidSide", channels, length, hq)
      .monoIn("in_left").monoIn("in_right")
      .monoIn("mid_in").monoIn("side_in")
      .monoOut("mid_out").monoOut("side_out")
      .monoOut("out_left").monoOut("out_right");
  }

  prepare(env: AudioEnv): void {}
  updateParameters(params: ParameterMap): void {}

  setPorts(inputs: InputPorts, outputs: OutputPorts): void {
    this.inLeft  = inputs.mono(0);
    this.inRight = inputs.mono(1);
    // ...
  }

  process(cables: CableIO): void {
    const l = cables.readMono(this.inLeft);
    const r = cables.readMono(this.inRight);
    cables.writeMono(this.midOut,  (l + r) * 0.5);
    cables.writeMono(this.sideOut, (l - r) * 0.5);
    // ... decode side ...
  }
}

registerModule(new MidSide());
```

**Why guest-side SDK, not host imports:** host-provided WASM imports
(`patches_host_read_mono`, etc.) would give the cleanest module code but each
call crosses the WASM sandbox boundary. At 4 inputs + 4 outputs and 48kHz,
that's ~384k trampoline calls/second per module — consuming most of the ~20µs
per-sample budget. The guest-side SDK's `@inline` methods compile to the same
raw memory ops as handwritten code, at zero cost.

### Current status

The spike crates are **excluded from the workspace build** (removed from
`Cargo.toml` members). They are experimental reference code. If later changes
to `patches-core` break compilation, that is expected and acceptable. The crates
have READMEs noting their experimental status and pointing to this ADR.

To resume this work, re-add the crates to the workspace members list and fix
any compilation issues against the current `patches-core` API.
