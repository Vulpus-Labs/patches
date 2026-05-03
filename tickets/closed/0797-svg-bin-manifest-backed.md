---
id: "0797"
title: patches-svg bin uses manifest-backed registry; centralise bundled JSON
priority: medium
created: 2026-05-03
epic: "E132"
adrs: ["0066"]
depends_on: ["0794"]
---

## Summary

After 0794, `patches-modules` reaches `patches-lsp` transitively only
through `patches-svg`'s lib dependency. Two-part fix:

1. Move the bundled `module-manifest.json` from
   `patches-lsp/data/` into `patches-manifest/data/`. Expose a
   `patches_manifest::bundled_manifest()` helper (with cached parse) so
   any consumer can fetch the manifest without `include_str!`-ing
   across crate boundaries.
2. Switch the `patches-svg` binary from
   `patches_modules::default_registry()` to
   `patches_manifest::bundled_manifest()` →
   `patches_manifest::static_registry::registry_from_manifest`. The
   binary only inspects descriptor surface; it never instantiates a
   module, so the describe-only registry is sufficient.
3. Move `patches-modules` to `[dev-dependencies]` in
   `patches-svg/Cargo.toml` (still used by the lib's `#[cfg(test)] mod
   tests`).

## Acceptance criteria

- [ ] `patches-manifest/data/module-manifest.json` exists; the LSP
      embed and the snapshot test in `patches-tools` retarget that
      path.
- [ ] `patches-svg` binary builds without `patches-modules` as a
      non-dev dependency.
- [ ] `cargo tree -p patches-lsp -e normal` no longer reaches
      `patches-modules` from any path.
- [ ] All existing patches-svg tests pass (lib's `#[cfg(test)]` use
      moves to dev-deps).
- [ ] `patches-svg` CLI smoke run (render a sample patch to SVG) still
      produces equivalent output vs. pre-change.

## Notes

- Centralising `bundled_manifest()` lets future consumers (vizia
  GUI, drum-sequencer UI, third-party tooling) skip the
  `include_str!` plumbing each time.
- The bundled manifest covers `default_registry()` only. Consumers
  that need plugin-supplied templates merge them in at startup
  (LSP's `rescan_modules`, future SVG plugin discovery, …).
