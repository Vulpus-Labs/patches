---
id: "E095"
title: Externalise patches-vintage as runtime-loadable plugin bundle
created: 2026-04-19
depends_on: ["E088", "E094"]
tickets: ["0569", "0570", "0571", "0572"]
---

## Goal

Ship `patches-vintage` as a single FFI bundle, loaded uniformly at
runtime by the LSP, `patches-player`, and the CLAP plugin. No vintage
modules appear in `default_registry()`; the only way to get them is
to point a consumer at the bundle on disk.

This is the end-to-end acceptance test for ADR 0044 + E088 + E094.

## Background

`patches-vintage` currently exposes VChorus, BBD, VFlanger,
VFlangerStereo, and a compander primitive, and is compiled as a normal
Rust library added to `default_registry()`. After this epic it is a
`cdylib` bundle that ships separately. Example patches using these
modules (e.g. `examples/poly_synth.patches` when it pulls in VChorus)
load the bundle at runtime in every host.

## Tickets

| ID   | Title                                                              | Priority | Depends on |
| ---- | ------------------------------------------------------------------ | -------- | ---------- |
| 0569 | Convert patches-vintage to cdylib with `export_modules!`           | high     | E088, 0563 |
| 0570 | Remove patches-vintage from default_registry; delete lib dep       | high     | 0569       |
| 0571 | Integration test: PluginScanner loads patches-vintage bundle       | high     | 0569, 0564 |
| 0572 | End-to-end: example patch runs in player, CLAP, and LSP via bundle | high     | 0570, 0571, 0565, 0566, 0567 |

## Definition of done

- `patches-vintage` builds as a `cdylib` and a `rlib` (tests remain on
  the rlib side). The `cdylib` exposes all public modules through
  `export_modules!` with per-module version symbols.
- `default_registry()` in `patches-modules` no longer depends on
  `patches-vintage`; the workspace still compiles cleanly.
- `PluginScanner` loads the built `patches-vintage.dylib`, reports every
  expected module name and version in its `ScanReport.loaded` list,
  and exactly one `Arc<Library>` backs all its builders.
- A patch using a vintage module:
  - runs under `patches-player --module-path target/debug path/to/patch`;
  - runs in the CLAP plugin when `module_paths` persisted state points
    at the built bundle;
  - is diagnostics-clean in VSCode when `patches.modulePaths` includes
    the bundle directory; hovering a vintage module shows its
    descriptor.
- Rescan verified: bumping `patches_module_version` on one module,
  rebuilding, and triggering rescan in CLAP + LSP replaces the builder
  and the change is observable via hover/descriptor.
- `cargo build`, `cargo test`, `cargo clippy` clean workspace-wide.
