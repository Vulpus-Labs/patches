---
id: "0268"
title: Shallow scan and dependency resolution (phases 1-2)
priority: high
created: 2026-04-07
---

## Summary

Implement the first two phases of the semantic analysis pipeline: shallow scan
(extract declaration names and kinds) and dependency resolution (build and
topo-sort the template dependency graph).

## Acceptance criteria

- [ ] `patches-lsp/src/analysis.rs` (or submodule) implements phase 1: scan the
      tolerant AST and extract a `DeclarationMap` containing all module instance
      names (with their type names and shape args), template names (with their
      signatures — params, in-ports, out-ports), and enum names (with members).
- [ ] Phase 2: build a dependency graph of templates referencing other templates.
      Topo-sort the graph. Emit a diagnostic for any cycles detected.
- [ ] Templates that reference unknown templates emit a diagnostic but do not
      block analysis of other declarations.
- [ ] Tests cover: file with no templates (trivial), file with independent
      templates, file with a template dependency chain, file with a template
      cycle.

## Notes

- A template `A` depends on template `B` if `A`'s body contains a module
  declaration whose type name matches `B`'s name.
- The `DeclarationMap` is the foundation for phases 3-4 and for completions.
- Epic: E049
