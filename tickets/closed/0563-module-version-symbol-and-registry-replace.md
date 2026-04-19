---
id: "0563"
title: Per-module version and version-aware Registry replacement
priority: high
created: 2026-04-19
epic: "E094"
---

## Summary

Add a per-module version to the FFI manifest and teach `Registry` to
prefer the highest-versioned builder per module name across rescans.

## Acceptance criteria

- [ ] `FfiPluginManifest` entry carries a `module_version: u32`
      (semver-packed `(major<<16)|(minor<<8)|patch`).
- [ ] `export_modules!` / `export_module!` grow a version argument
      (or read an attribute) and emit it into each entry.
- [ ] `Registry::register_builder` (or a new sibling) accepts a
      version; when re-inserting under an existing name it replaces
      only if the new version is strictly greater. Equal or lower is
      a no-op that the caller can observe.
- [ ] Replacement does *not* affect any already-built `ExecutionPlan`
      (covered by 0562).
- [ ] Unit tests: insert v1 → v2 replaces; v2 → v1 does not; same
      version is skipped.

## Notes

ADR 0044 §1. Version skew across a single bundle is allowed — each
entry ships its own version.
