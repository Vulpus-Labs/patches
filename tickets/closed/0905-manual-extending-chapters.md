---
id: "0905"
title: Manual — extending Patches chapters
priority: medium
created: 2026-05-17
epic: E149
---

## Summary

Write the two new Extending Patches chapters (folded ABI reference,
native-plugin walkthrough) and audit the existing
`implementing-modules.md`.

## Acceptance criteria

- [ ] `docs/src/extending-abi.md` — fold deleted
      `abi/descriptor-schema.md` and `abi/wire-formats.md` into one
      reference chapter. Cover the FFI vtable shape, descriptor
      schema (module type, ports, params, shape args), and wire
      formats for control-rate data. Audit against
      `patches-ffi-common/` and `patches-ffi/` for drift since the
      originals were written. Flag in the chapter intro that the
      ABI is not yet considered stable.
- [ ] `docs/src/extending-native-plugin.md` — worked example using
      `test-plugins/gain/` and / or `test-plugins/conv-reverb/`.
      Project layout, `cdylib` Cargo setup, descriptor definition,
      process function, building, loading via `--module-path` or
      global bundle dirs (E148). Audit `test-plugins/` before
      writing — pick a plugin that's still building.
- [ ] `docs/src/implementing-modules.md` — audit existing chapter
      for accuracy. Should describe in-tree Rust module authoring
      (Module trait, ModuleDescriptor, port declarations, state
      handling). Cross-reference the module documentation standard
      from CLAUDE.md. Update for any post-ADR-0072 fusion-related
      API changes.

## Notes

- Deleted sources: `git show <commit>:docs/src/abi/descriptor-schema.md`
  and `docs/src/abi/wire-formats.md`.
- FFI design context in memory: `project_ffi_design`. Worth
  checking before drafting extending-abi.
- ABI externalisation roadmap (memory: `project_abi_externalization`)
  is forward-looking — out of scope here.
- `test-plugins/CLAUDE.md` if present has plugin-authoring notes.
- Bundle directory resolution order documented in ADR 0075 (per
  E148). Cross-reference for `--module-path` /
  `PATCHES_PLUGIN_PATH` / global config.
