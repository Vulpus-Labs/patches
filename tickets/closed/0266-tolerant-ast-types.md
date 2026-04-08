---
id: "0266"
title: Tolerant AST types
priority: high
created: 2026-04-07
---

## Summary

Define the tolerant AST types for the LSP analysis pipeline. These mirror the
structure of `patches-dsl::ast` but use `Option<T>` for fields that may be
absent due to parse errors in incomplete source.

## Acceptance criteria

- [ ] `patches-lsp/src/ast.rs` defines tolerant equivalents of the key DSL AST
      types: `File`, `Patch`, `Template`, `ModuleDecl`, `Connection`, `PortRef`,
      `Arrow`, `EnumDecl`, `ParamEntry`, `ShapeArg`, `Scalar`, `Value`, and
      supporting types.
- [ ] Fields that can be absent mid-edit are `Option<T>`: module type name,
      port label, arrow direction, connection endpoints, template name, etc.
- [ ] All nodes carry a `Span` (byte offset range into the source).
- [ ] Fields that are inherently optional in the grammar (shape block, param
      block, port index, arrow scale) remain `Option<T>` as in the strict AST.
- [ ] Types derive `Debug` and `Clone`.
- [ ] No dependency on `patches-dsl` — these are independent types.

## Notes

- The tolerant AST is `pub(crate)` — internal to `patches-lsp`.
- Don't over-optionalise. A `ModuleDecl` with no name at all is better
  represented as absent from the parent list than as a node with `name: None`.
  Use `Option` for fields where the node is identifiable but incomplete
  (e.g. `module foo :` — name present, type name missing).
- Epic: E049
