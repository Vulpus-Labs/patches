---
id: "0507"
title: Split patches-dsl expand/mod.rs by concern
priority: medium
created: 2026-04-16
---

## Summary

`patches-dsl/src/expand/mod.rs` is 1484 lines. `composition` and
`connection` already live in siblings; the remaining file mixes
error types, warning types, scope/namespace helpers, and the
recursive template-expansion orchestration.

## Acceptance criteria

- [ ] `ExpandError`, `Warning`, `ExpandResult`, `param_type_name`,
      and any fmt impls move to `expand/error.rs`.
- [ ] Scope/namespace helpers (scope stack, song/pattern resolution)
      move to `expand/scope.rs`.
- [ ] `expand/mod.rs` retains the `expand` entry point and template
      recursion/parameter binding, under ~600 lines.
- [ ] `StructuralError` re-export unchanged at crate root.
- [ ] `cargo build -p patches-dsl`, `cargo test -p patches-dsl`,
      `cargo clippy` clean.

## Notes

E086. No behaviour change.
