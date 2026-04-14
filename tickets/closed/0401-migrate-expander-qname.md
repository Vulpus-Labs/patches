---
id: "0401"
title: Migrate DSL expander to QName
priority: medium
created: 2026-04-14
---

## Summary

Replace `qualify()` / `child_ns()` string concatenation in the
`patches-dsl` expander with `QName` operations. Scope maps
(`NameScope`) store `QName` values instead of slash-joined strings.

## Acceptance criteria

- [ ] `qualify` and `child_ns` helpers removed; callers use
      `QName::child` and `QName::bare`.
- [ ] `NameScope` fields for songs, patterns, and module instances hold
      `QName`.
- [ ] Expander output surfaces (`FlatPatch` module IDs, connection
      endpoints, `SongDef` name, emitted pattern names) expose `QName`;
      downstream consumers will be migrated in 0402.
- [ ] Existing expander tests pass; no change in observable
      serialisation beyond via `Display`.

## Notes

Depends on 0400. Do not change `FlatPatch` field types yet if that
cascade is large — instead, expose a thin `Display`-based conversion at
the crate boundary and let 0402 flip the types.
