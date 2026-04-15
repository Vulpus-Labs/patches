---
id: "0448"
title: Consolidate StructuralCode code/label tables
priority: low
created: 2026-04-15
---

## Summary

`patches-dsl/src/structural.rs:56-82` defines `as_str()` and `label()`
as parallel match statements over 22 `StructuralCode` variants — 42
lines maintained in lockstep. Adding a variant means updating two
places and a silent drift is easy to introduce.

Consolidate to one source of truth: either a `&[(StructuralCode, &str,
&str)]` table with `as_str` / `label` looking up, or a declarative
macro that generates both match arms from one list.

## Acceptance criteria

- [ ] Adding a new `StructuralCode` variant requires updating exactly
      one place.
- [ ] Existing code and label strings are byte-for-byte unchanged
      (verify via diagnostic-output tests).
- [ ] `cargo test -p patches-dsl`, `cargo clippy` clean.

## Notes

Part of E082. Pure internal refactor. Lowest priority — do last or
bundle into another ticket's cleanup commit.
