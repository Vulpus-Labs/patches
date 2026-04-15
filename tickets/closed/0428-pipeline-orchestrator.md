---
id: "0428"
title: Pipeline orchestrator with staged entry points
priority: high
created: 2026-04-15
---

## Summary

Introduce a pipeline orchestrator (likely `patches-dsl::pipeline` — a
new module within patches-dsl, or a thin crate if cross-crate types
demand it) that exposes the stages from ADR 0038 as named, composable
entry points. Each stage takes the previous stage's output and a
policy choice (fail-fast vs accumulate) and returns either the next
artifact or an aggregated diagnostics bundle.

Stages:

1. `load` — root path → `LoadResult` (+ `LoadError`s)
2. `parse` — `LoadResult` → pest `File`s (+ `ParseError`s)
3. `expand` — pest `File`s → `FlatPatch` (+ `ExpandError`s, mechanical)
4. `structural` — `FlatPatch` → `FlatPatch` (+ `StructuralError`s)
5. `bind` — `FlatPatch` → `ModuleGraph` (+ `InterpretError`s)

## Acceptance criteria

- [ ] Each stage exposed as a function with a clearly-typed input and
      output; no hidden cross-stage coupling.
- [ ] A `run_all` convenience runs stages 1–5 with a policy (fail-fast
      or accumulate) and returns the deepest artifact produced.
- [ ] Stage 4 (tree-sitter) is **not** part of this orchestrator;
      LSP owns that fallback alongside the orchestrator call.
- [ ] Player and CLAP can be rewritten to call the orchestrator in a
      follow-up ticket without changing stage internals.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

Depends on 0426, 0427. Don't over-abstract — resist the urge to make
stages generic over AST type to accommodate the tree-sitter path. That
was explicitly rejected in ADR 0038: TS stays parallel.
