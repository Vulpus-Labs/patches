---
id: "E088"
title: Multi-module FFI plugin bundles and drum bundle
created: 2026-04-16
tickets: ["0492", "0493", "0494", "0495", "0496"]
---

## Goal

Allow a single FFI plugin `.dylib`/`.so`/`.dll` to expose multiple
module types from one entry symbol, so module families that share DSP
code (drums, future stereo effects suites, etc.) can ship as one
artefact with one statically-linked copy of their shared dependencies.

After this epic:

- `patches-ffi` exposes `FfiPluginManifest` and `export_modules!(...)`,
  with `export_module!(T)` preserved as a thin shim.
- ABI bumps to version 2; `gain`, `conv-reverb`, and `gain-wasm` test
  plugins recompile unchanged.
- `load_plugin` returns `Vec<DylibModuleBuilder>`; the scanner
  flattens bundles transparently.
- A `drums-bundle` test plugin packages all eight drum modules
  (`Kick`, `Snare`, `ClapDrum`, `ClosedHiHat`, `OpenHiHat`, `Tom`,
  `Claves`, `Cymbal`) into one `.dylib`.
- An integration test loads the drum bundle, instantiates two modules
  from it, and verifies they share one library handle.

## Background

ADR 0039 documents the design. Today every plugin links its own copy
of `patches-dsp`; the eight drum modules would duplicate
`DecayEnvelope`, `PitchSweep`, `MetallicTone`, `BurstGenerator`,
`SvfKernel`, and the noise PRNG eight times. The bundle ABI removes
that duplication and matches how mature plugin formats (VST3, CLAP,
AU) ship instrument banks.

This epic delivers the *capability*. Whether to actually pull drums
out of `patches-modules`'s `default_registry()` is a separate
decision (suggest deferring until plugin distribution becomes a real
user workflow).

## Tickets

| ID   | Title                                              | Priority | Depends on |
| ---- | -------------------------------------------------- | -------- | ---------- |
| 0492 | FfiPluginManifest type and ABI v2 bump             | high     | —          |
| 0493 | export_modules! macro and export_module! shim      | high     | 0492       |
| 0494 | Loader and scanner support for multi-module bundles | high     | 0492, 0493 |
| 0495 | drums-bundle test plugin                           | medium   | 0493, 0494 |
| 0496 | Integration test: shared library handle            | medium   | 0495       |

## Definition of done

- ABI v2 fully replaces v1; `ABI_VERSION = 2` everywhere.
- `export_module!(T)` callers compile with no source change.
- `load_plugin` returns a non-empty vec on success; scanner flattens.
- Duplicate `module_name` within a single manifest is a load-time
  error reported per-bundle.
- `drums-bundle.dylib` exposes all eight drum module names; loaded
  modules pass each crate's existing trigger-response tests.
- An integration test asserts two `DylibModuleBuilder`s from one
  bundle share one `Arc<libloading::Library>` (refcount > 1).
- `cargo build`, `cargo test`, `cargo clippy` clean across the
  workspace, with no `unwrap()`/`expect()` added to library code.
