---
id: "0452"
title: Split NameScope into NameResolver and SectionTable
priority: medium
created: 2026-04-15
---

## Summary

`NameScope` in `patches-dsl/src/expand.rs` (lines 532–644) conflates two
concerns: a resolution cache for unqualified song/pattern names
(lines 611–630) and a section-definition visibility table
(lines 603–609). These are different axes — lexical scoping vs.
definition lookup — and mixing them blocks reasoning about either.

## Acceptance criteria

- [ ] New `NameResolver` owns song/pattern lookup by (possibly
      unqualified) name, with parent-chain walk for nested scopes.
- [ ] New `SectionTable` owns section-definition visibility.
- [ ] Call sites consume whichever they need; a thin `Scope` wrapper
      may hold both if helpful.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

E083. Enables future scope work (alias isolation, private sections)
without entangling name lookup.
