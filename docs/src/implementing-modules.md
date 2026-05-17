# Implementing modules

This chapter is for adding a new module to the in-tree
`patches-modules` crate. The trait, descriptor templates, and
audio-thread contract are identical to the [native plugin
SDK](extending-native-plugin.md) — only the packaging differs.

If you want the worked example, read the native-plugin chapter
first. The 88-line `Gain` module there is the smallest complete
module, and the Rust is the same whether it ships in-tree or as a
loadable bundle. This chapter covers what's different about in-tree
modules.

## What changes for in-tree

| Concern        | Native plugin                       | In-tree module                                                            |
| -------------- | ----------------------------------- | ------------------------------------------------------------------------- |
| Crate          | New `cdylib` outside the tree       | New file under `patches-modules/src/`                                     |
| Dependency     | `patches-sdk = "0.7"`               | `patches-core`, `patches-dsp`                                             |
| Registration   | `export_plugin!` macro              | `Registry::register::<Module>()` in `patches-modules::default_registry()` |
| Loading        | Discovered via bundle dirs          | Compiled into the host                                                    |
| Distribution   | Built separately, shipped as `.pxm` | Ships with the main release                                               |
| Docs           | Source comments only                | Source comments → manual's module reference (see below)                   |

The `Module` trait, `ModuleDescriptorTemplate`, parameter handles
via `module_params!`, and the audio-thread rules are identical.
Anything you'd write inside a plugin's `impl Module` goes into an
in-tree module unchanged.

## Steps for a new in-tree module

1. **Add a file.** `patches-modules/src/my_module.rs`. Use one of
   the existing modules as a starting template — `oscillator.rs`,
   `vca.rs`, and `glide.rs` are good shapes.
2. **Implement `Module`.** Same surface as the SDK example. Use
   types from `patches-core` directly instead of `patches-sdk`
   re-exports (`patches_core::cables::*`,
   `patches_core::modules::*`, `patches_core::param_frame::ParamView`).
3. **Declare the module.** Add `pub mod my_module;` to
   `patches-modules/src/lib.rs` (and `pub use my_module::MyModule;`
   if you want it re-exported by name).
4. **Register it.** In `default_registry()` in
   `patches-modules/src/lib.rs`, add
   `r.register::<MyModule>();`. The order in this function controls
   nothing user-visible; group with related modules for readability.
5. **Document it.** Add a doc comment in the source per the module
   documentation standard (covered below). The manual's module
   reference is generated from these comments.
6. **Test it.** Use the `ModuleHarness` test support in
   `patches-core::test_support` to exercise the module without a
   running audio engine.

## DSP code lives in `patches-dsp`

Pure DSP algorithms — filter kernels, ADSR cores, oversampling,
delay lines, noise PRNGs — belong in `patches-dsp`, not
`patches-modules`. The split:

- **`patches-dsp`** — algorithm. Takes raw `f32` inputs, returns
  `f32` outputs. No `patches-core`, no module protocol, no
  allocation. Pure functions or state objects.
- **`patches-modules`** — protocol glue. Wraps a DSP kernel as a
  Patches module: declares ports and parameters, reads from the
  cable pool, calls the kernel, writes back.

Most existing modules are thin wrappers — the SVF filter module
holds an `SvfCore` from `patches-dsp` and threads cable values
through it. That separation lets the DSP code be tested in
isolation and reused by multiple modules (e.g. mono and poly
variants both reach into the same kernel).

## Module documentation standard

Every module under `patches-modules/src/` carries a doc comment
(`///` on the struct or `//!` at file level) in a standard form.
This comment is the **source of truth** for the manual's module
reference under `docs/src/modules/`.

The format, condensed (full spec in `CLAUDE.md`):

```rust
/// Brief one-line description.
///
/// Extended description (algorithm, CV behaviour, etc.).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `name` | mono/poly | What it does |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `name` | mono/poly | What it does |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `name` | float/int/bool/enum | range | `default` | What it does |
```

Port names must match the strings in the module's descriptor
template. Indexed ports use `port[i]` notation with a note on the
range. Omit sections that don't apply.

When you change a module's ports or parameters, update this
comment in the same commit. The manual regeneration tooling reads
the comments as authoritative.

## Audio-thread rules

Same as for native plugins, same as for the engine itself:

- No allocation on the audio thread (`process`,
  `update_validated_parameters`, `set_ports`).
- No blocking (no mutexes, no I/O, no syscalls).
- No unbounded loops.
- `panic = "unwind"` only — the tick-boundary catch_unwind
  requires it.

The `patches-engine` integration tests run with the
`audio-thread-allocator-trap` feature on, which traps any heap
allocation on the audio thread and panics — useful for catching
accidental allocation in `process` during development. The harness
in `patches-core::test_support` exposes a `Harness::tick` that
matches the engine's calling convention closely enough that
allocation bugs in your module's hot path will surface there.

## Cross-reference

- [Writing a native plugin](extending-native-plugin.md) — the
  worked example. Read this for the trait surface walk-through.
- [Native plugin ABI](extending-abi.md) — the underlying C ABI.
  Not needed for in-tree modules (no FFI boundary) but useful if
  you want to understand how descriptors and parameter frames
  cross the same trait surface when packaged as a bundle.
- [Engine internals](engine-internals.md) — what the host does
  with your module: planner, cable pool, plan handoff, periodic
  updates.
