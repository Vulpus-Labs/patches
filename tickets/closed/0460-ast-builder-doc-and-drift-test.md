---
id: "0460"
title: ast_builder.rs module docstring and drift test docstring
priority: low
created: 2026-04-15
---

## Summary

Two small documentation gaps in `patches-lsp`:

1. `ast_builder.rs` (1441 lines) is misnamed — it lowers pest parse
   trees into a tolerant AST with diagnostics, not "builds" an AST.
   Either rename to `tolerant_ast.rs` or add a module-level docstring
   explaining the role.
2. The drift test at `ast.rs:463` (`drift_maps_compile`) compiles only
   when all DSL enum variants are handled; it asserts nothing at
   runtime. Without a docstring, a contributor seeing a compile error
   in the drift module will not understand what the test guards.

## Acceptance criteria

- [ ] `ast_builder.rs` has a module docstring clarifying its role, or
      is renamed with call-sites updated.
- [ ] Drift test / module in `ast.rs` has a docstring explaining the
      exhaustiveness guarantee.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E083. Documentation only.
