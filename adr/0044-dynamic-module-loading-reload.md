# ADR 0044 — Dynamic module loading and reload

**Date:** 2026-04-19
**Status:** Proposed

---

## Context

ADR 0025 established the FFI plugin format. ADR 0039 / E088 extends it
to multi-module bundles. What is still missing: a coherent *runtime*
story for discovering, versioning, and reloading external modules
across the three consumers that host a `Registry` — `patches-player`,
`patches-clap`, and `patches-lsp` (with its VSCode client).

Today each consumer either uses `default_registry()` or calls
`scan_plugins` once against a hard-coded directory. There is no:

- configurable list of search paths, per consumer;
- module-level version so newer builds can shadow older ones;
- rescan command for long-lived processes (LSP server, CLAP plugin
  instance);
- defined dylib lifetime contract — nothing prevents `dlclose` while
  a `Box<dyn Module>` with a vtable pointing into that dylib is still
  live in an executing `ExecutionPlan`.

Patches-vintage is the forcing function: we want to externalise it as
a single shippable bundle (VChorus, BBD, VFlanger, VFlangerStereo,
compander, …) that loads uniformly into the player, CLAP plugin, and
LSP-backed editor.

## Decision

### 1. Module + ABI versioning

Every bundle exports, in addition to the `FfiPluginManifest`:

- `patches_abi_version: u32` — already present, gates layout
  compatibility. Loader rejects mismatches before touching the
  manifest.
- `patches_module_version: u32` per module entry in the manifest —
  semver-packed `(major<<16)|(minor<<8)|patch`. A single bundle may
  ship many modules at different versions.

The `Registry` keeps the highest-versioned builder per module name.
On rescan, a newer version replaces the older; same or lower version
is skipped. Replacements only affect *subsequent* `compile()` calls;
existing plans keep their original builders.

### 2. Dylib lifetime: reference-counted handle

`DylibModuleBuilder` and every `DylibModule` instance hold a clone of
`Arc<libloading::Library>`. The library is unloaded only when the last
`Arc` drops. Concretely:

- Registry replacement drops its `Arc`, but any live plan still holds
  instances → library stays mapped.
- Plan drop releases all instance `Arc`s.
- When both registry and all plans release, the library unloads.

This makes v1 and v2 of the same module safely coexist in memory for
as long as needed.

### 3. Reload semantics: hard-stop

No lock-free hot swap of live instances. Reloading is always:

1. Stop audio (CLAP `stop_processing`/`deactivate`; player not
   applicable — it exits).
2. Drop current `ExecutionPlan` (runs instance destructors while
   dylibs are still mapped).
3. Rescan: for each path, load bundles, compare versions, update
   `Registry`.
4. Recompile the patch against the new `Registry`.
5. Resume audio.

Softer double-buffered hot-swap is deferred; the hard-stop path is
the only supported reload contract. This keeps vtable lifetime rules
trivially correct.

### 4. Scanner shared contract

A shared type (in `patches-ffi` or a new `patches-plugins` crate)
exposes:

```rust
pub struct PluginScanner { pub paths: Vec<PathBuf> }

pub struct ScanReport {
    pub loaded:     Vec<LoadedModule>,   // name, version, path
    pub replaced:   Vec<Replacement>,    // older superseded by newer
    pub skipped:    Vec<SkipReason>,     // lower version, abi mismatch, duplicate
    pub errors:     Vec<(PathBuf, String)>,
}

impl PluginScanner {
    pub fn scan(&self, registry: &mut Registry) -> ScanReport;
}
```

Consumers differ only in *where paths come from* and *when `scan` is
called*.

### 5. Per-consumer integration

| Consumer | Path source | Scan trigger | Rescan |
|---|---|---|---|
| `patches-player` | `--module-path` CLI flag (repeatable) | once before compile | n/a (restart) |
| `patches-clap` | plugin state (persisted) | on `activate`/plugin open | GUI rescan button → hard-stop flow |
| `patches-lsp` | `workspace/configuration` (`patches.modulePaths`) | on init and on config change | custom command `patches/rescanModules` |
| VSCode client | `patches.modulePaths` setting | — | command `patches.rescanModules` → LSP custom command |

LSP performs scan in-process (subprocess isolation deferred; see
Consequences). CLAP serialises paths via its state extension.

### 6. Process isolation (deferred)

Running dylib `describe()` calls in-process exposes LSP/CLAP/player
to plugin crashes. A subprocess scanner that returns cached
`(name, shape_key) -> ModuleDescriptor` maps over IPC is desirable
but is out of scope for this ADR. The hard-stop reload model does not
require it; a future ADR can add it.

## Consequences

**Positive**

- One coherent plugin lifecycle across all three consumers.
- Version shadowing enables in-place upgrades without touching disk
  layout.
- `Arc<Library>` rule makes ordering-of-drops bugs impossible to write
  accidentally.
- Hard-stop reload keeps the design boringly correct; double-buffered
  hot-swap can be layered on later under the same `ScanReport` API.

**Negative**

- CLAP rescan interrupts audio — users must stop/start playback.
  Acceptable for a dev-oriented editor workflow.
- No crash isolation v1: a buggy plugin can kill the LSP or CLAP host.
  Mitigation: ABI version gate rejects skew; subprocess scanner in a
  future ADR.
- `TypeId`/`Any` across dylibs still unreliable — reinforces existing
  rule that FFI boundary is `#[repr(C)]` + primitives/pointers only.
- Duplicated statics per loaded version (OnceCells, thread_locals)
  are user-visible only if plugins install global hooks; keep plugin
  init side-effect-free by convention.

## Related

- ADR 0025 — Dynamic module loading (FFI format)
- ADR 0039 — Multi-module plugin bundles
- ADR 0027 — WASM module loading (alternative runtime)
- E088 — Multi-module FFI plugin bundles (prerequisite)
