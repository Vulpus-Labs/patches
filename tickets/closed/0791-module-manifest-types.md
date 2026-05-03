---
id: "0791"
title: ModuleManifest serde types
priority: medium
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0785"]
---

## Summary

Define `ModuleManifest`, `ModuleManifestEntry`, and the owned mirror
of `ModuleDescriptorTemplate` (with `String` instead of `&'static str`,
`Vec` instead of `&'static [..]`). Add serde derives. Provide `From`
impls between static and owned forms.

`schema_version: u32` and `generator: GeneratorInfo` (tool name,
git rev, generation timestamp) are part of the manifest envelope.

## Acceptance criteria

- [ ] Owned mirror types in a dedicated module
      (`patches-core/src/modules/manifest.rs` or a new
      `patches-manifest` crate — pick whichever keeps LSP's eventual
      dep graph cleanest).
- [ ] Serde round-trip test: build a manifest from
      `default_registry()` templates, serialize, deserialize, assert
      equality.
- [ ] `From<&'static ModuleDescriptorTemplate> for OwnedTemplate`
      implemented.
- [ ] `schema_version` constant defined; bump policy documented in
      ADR 0066.

## Notes

- If a new `patches-manifest` crate is created, it should depend
  only on `patches-core`'s template types (no audio-engine deps), so
  LSP can depend on it without pulling in `patches-modules`.
- Decide between in-core and new crate based on whether
  `patches-core` is acceptable in LSP's dep graph (it already is).
