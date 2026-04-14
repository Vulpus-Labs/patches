---
id: "0402"
title: Migrate FlatPatch and interpreter to QName
priority: medium
created: 2026-04-14
---

## Summary

Change `FlatPatch` module IDs, connection endpoints, pattern bank keys,
and song names from `String` to `QName`. Update `patches-interpreter` to
consume the structured form without re-splitting strings.

## Acceptance criteria

- [ ] `FlatPatch` fields carrying qualified identifiers use `QName`.
- [ ] `patches-interpreter` builds `ModuleGraph`, pattern bank, and song
      tables from `QName` keys; any place that previously parsed
      `"ns/name"` strings uses `QName::path` / `QName::name` directly.
- [ ] Pattern bank alphabetical indexing (ADR 0029) sorts by `Display`
      of `QName` — stable with respect to prior behaviour for
      unqualified names.
- [ ] Integration tests and fixtures pass unchanged.

## Notes

Depends on 0401. Most consumer changes are mechanical.
