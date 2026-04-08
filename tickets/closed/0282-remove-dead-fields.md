---
id: "0282"
title: Remove dead fields and unused parameters in analysis types
priority: low
created: 2026-04-08
---

## Summary

Several analysis types carry `_`-prefixed `pub(crate)` fields that are collected
but never read: `ModuleInfo._param_names`, `EnumInfo._name`, `EnumInfo._members`,
`EnumInfo._span`, `DependencyResult._sorted`. The function `complete_ports` takes
an unused `_registry: &Registry` parameter.

Remove or use these. If they're intended for future work, file a ticket for that
work and remove them now.

## Acceptance criteria

- [ ] No `_`-prefixed `pub(crate)` fields remain on types in `analysis.rs`.
- [ ] No unused parameters on `pub(crate)` functions.
- [ ] If any removed field is needed for a planned feature, a follow-up ticket
      is filed noting that the field will need to be re-added.
- [ ] `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.
