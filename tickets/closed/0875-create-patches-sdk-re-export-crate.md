---
id: "0875"
title: Create patches-sdk re-export crate; migrate bundles to consume it
priority: medium
created: 2026-05-11
---

## Summary

Add a `patches-sdk` crate to the workspace. Its job: present a single
import surface for external module authors via `cargo add patches-sdk`.
Re-exports:

- `patches-core`: Module trait, descriptors, ports, cables, params,
  ParamFrame, BuildError, AudioEnvironment, InstanceId, ModuleShape,
  Registry + registration helpers (from `patches_core::registry::*`
  post-ticket-0889), `test_support` (feature-gated).
- `patches-ffi-common`: `export_modules!` macro.

**Deliberately does NOT re-export `patches-dsp`.** Module authors who
want DSP kernels add `patches-dsp` as a git dep separately. Keeps the
SDK surface narrow and the crates.io publish set at three crates per
[ADR 0073](../../adr/0073-monorepo-split-into-successor-repos.md).

Migrate the four existing SDK consumers (patches-vintage,
patches-drums, patches-fft-bundle, test-plugins/*) to depend on
`patches-sdk` for the trait + macro surface; they keep their direct
`patches-dsp` dependency for kernels. This is the contract test — if
a bundle needs a non-dsp symbol not re-exported, the SDK surface
needs deliberate extension or the bundle needs refactor.

## Acceptance criteria

- [x] `patches-sdk` crate created with documented `pub use` blocks.
- [x] `test-support` feature on patches-sdk propagates to
      `patches-core/test-support`.
- [x] Every public type a current bundle uses is re-exported. No
      bundle imports `patches-core`, `patches-dsp`,
      `patches-registry`, or `patches-ffi-common` directly after
      this lands (verified by manifest inspection).
- [x] patches-vintage Cargo.toml: deps reduced to `patches-sdk` +
      `patches-dsp`.
- [x] patches-drums Cargo.toml: same (sdk + dsp).
- [x] patches-fft-bundle Cargo.toml: `patches-sdk` + `patches-dsp` +
      `patches-fft-harness` + `patches-io` + `rtrb` (harness and io
      are not in SDK).
- [x] test-plugins/*/Cargo.toml: `patches-sdk` only.
- [x] `cargo test -p patches-vintage`, `cargo test -p patches-drums`,
      `cargo test -p patches-fft-bundle` green.
- [x] Doc comment on `patches-sdk/src/lib.rs` explains the contract:
      anything reachable from this crate's public API is supported.
- [x] `#![warn(missing_docs)]` on patches-sdk; all `pub use` blocks
      grouped and documented (one section per source crate).

## Notes

Depends on:

- 0889 (registry merged into core — simpler re-export surface)
- 0873 (patches-drums exists)
- 0874 (patches-fft-bundle exists)

Should not include:

- `patches-fft-harness` — bundle-author-internal, lives in patches-fft
  repo. Not foundation, not SDK.
- Host-side types (engine, planner, observation). Modules don't see
  these.
- DSL types (patches-dsl). Modules don't author DSL.

`patches-sdk` becomes the user-facing crate in its own repo post-cut
(ticket 0881). Today it's a workspace crate; the cut is mechanical.

Pattern: keep `patches-sdk` thin — a re-export crate with a macro
forwarding shim if needed. No new types. Surface stability is its
value proposition.

## Implementation notes

- The `export_modules!`, `export_plugin!`, and
  `export_plugin_with_hash_override!` macros in `patches-ffi-common`
  previously hard-coded `::patches_core::...` absolute paths in their
  expansions, forcing every macro consumer to list `patches-core` as
  a direct dep. Rewrote the macro bodies to use `$crate::...` and
  added re-exports for `Module`, `ModuleShape`, `cable_pool`, and
  `cables` at the top of `patches-ffi-common/src/lib.rs`. One
  non-macro reference in a `#[cfg(test)]` block at sdk.rs:981 kept
  the absolute path.
- Test-plugins reach deeper than module bundles (`port_frame`,
  `sdk::PluginInstance`, `abi::Handle`, `descriptor_hash`). Those
  are re-exported through `patches-sdk` with doc comments flagging
  them as advanced surfaces; the supported public path for normal
  module authors stays the trait + macro at crate root.
- `pub use patches_core::*;` plus per-submodule re-exports
  (`cables`, `modules`, `param_frame`, `parameter_map`,
  `build_error`, `cable_pool`, `module_params`, `registry`) covers
  the existing call-site shape so bundles only needed a
  `s/patches_core/patches_sdk/` rewrite, not a refactor.
- Bundles' `Cargo.toml` no longer reference `patches-core` or
  `patches-ffi-common` directly; transitive resolution through
  `patches-sdk` handles what macro expansion needs.
