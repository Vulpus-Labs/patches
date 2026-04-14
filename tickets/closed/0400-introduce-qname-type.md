---
id: "0400"
title: Introduce QName type
priority: medium
created: 2026-04-14
---

## Summary

Add a `QName` type (tuple-structured qualified identifier) in a location
shared by `patches-dsl` and `patches-interpreter`. Per ADR 0034.

## Acceptance criteria

- [ ] `QName { path: Vec<String>, name: String }` with constructors
      `bare`, `child`, and predicate `is_bare`.
- [ ] `Display` joins `path` and `name` with `/`.
- [ ] `Eq`, `Hash`, `Clone`, `Debug` derived; ordering via `Display`
      equivalence for deterministic bank sorting.
- [ ] Unit tests covering construction, child extension, display, and
      equality.

## Notes

Preferred location: `patches-core` (no new crate dependency needed by
downstream consumers). Confirm before adding — `patches-core` must
remain backend-agnostic.
