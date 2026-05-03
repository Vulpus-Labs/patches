---
id: "E132"
title: Static descriptor templates and ModuleManifest externalization
created: 2026-05-02
tickets: ["0785", "0786", "0787", "0787a", "0787b", "0788", "0789", "0789c", "0790", "0791", "0792", "0793", "0794", "0795", "0796"]
adrs: ["0066"]
related: ["E126"]
---

## Goal

Implement [ADR 0066](../../adr/0066-static-descriptor-templates-and-manifest.md):
replace runtime `Module::describe(shape)` with a compile-time
`ModuleDescriptorTemplate` const per module type, introduce a
serializable `ModuleManifest` produced by a CLI, and rewire LSP and
the FFI ABI to consume the template/manifest. Drops `patches-modules`
from the LSP dependency graph and removes per-instance descriptor
allocation across the FFI.

## Scope

1. `ModuleDescriptorTemplate` types (static + owned mirror) in
   `patches-core`. `build_channels(u32) -> ModuleDescriptor`. Internal
   multi-axis representation; single-axis surface today.
2. `Module::TEMPLATE` associated const with defaulted `describe()`.
3. Migrate every module in `patches-modules` to `const TEMPLATE`.
   Three batches: single-channel, channel-aware, poly-fixed.
4. Remove `Module::describe` trait method once all modules migrated.
5. `ModuleManifest` schema + serde. Owned mirror types.
6. Extend `patches-manifest` binary with `--json` to emit manifest;
   wire CI regeneration on module changes.
7. `patches-lsp` consumes manifest at startup; drop `patches-modules`
   dep; replace registry.describe() call sites.
8. FFI ABI bump (combined with ADR 0060): replace `describe` vtable
   with `module_template`. Update SDK macro and test plugins.

## Acceptance

- ADR 0066 implemented end-to-end across core, modules, planner,
  LSP, FFI.
- `patches-lsp/Cargo.toml` no longer depends on `patches-modules`.
- `cargo test` and `cargo clippy` pass on inner-loop subset and
  full workspace.
- `patches-manifest --json` emits a valid manifest consumed by LSP
  with byte-identical descriptor results vs. today's runtime path.
- Test plugin loaded over FFI uses `module_template` vtable; ABI
  version 8 enforced at load.

## Sequencing

- E126 (ADR 0060) is complete; this epic builds on the reduced
  `ModuleShape { channels }` already in tree.
- Within E132: 0785 → 0786 → (0787, 0787a, 0787b, 0788, 0789, 0789c parallel)
  → 0790 → (0791 in parallel with migrations) → 0792 → 0793 → 0794.
  FFI track 0795 → 0796 starts after 0790. ABI bump 7 → 8 stands
  alone.
