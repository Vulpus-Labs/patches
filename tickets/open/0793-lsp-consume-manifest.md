---
id: "0793"
title: LSP loads ModuleManifest at startup
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0792"]
---

## Summary

`patches-lsp` loads `ModuleManifest` at startup (bundled JSON path
configurable via env or LSP init option, default to embedded bytes).
All sites that today call `registry.describe(name, shape)` look up
the manifest entry and call `entry.template.build_channels(channels)`.

`patches-modules` is **not yet** dropped from `Cargo.toml` — that
lands in 0794 once this is proven correct in parallel.

## Acceptance criteria

- [ ] LSP loads the manifest at workspace init; failure to load is a
      clear startup error.
- [ ] All `registry.describe(...)` call sites in
      `patches-lsp/src/analysis/` switch to manifest lookup.
- [ ] Existing LSP tests pass unchanged.
- [ ] Add a smoke test asserting that a manifest-derived descriptor
      matches the registry-derived one for a sample module set
      (regression net for staleness).

## Notes

- Embed the JSON via `include_str!` from
  `patches-lsp/data/module-manifest.json` so LSP has a fallback when
  no external manifest is configured.
- Keep registry-based path alive temporarily as a feature-flagged
  fallback to make rollback trivial; remove in 0794.
