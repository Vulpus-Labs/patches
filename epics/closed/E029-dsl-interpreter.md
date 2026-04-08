---
id: "E029"
title: DSL interpreter — Stages 2 & 3 (expander + graph builder)
created: 2026-03-19
tickets: ["0140", "0141", "0143", "0144"]
---

## Summary

With Stage 1 complete (E028 — PEG parser), this epic delivers the rest of the
compilation pipeline described in ADR 0005:

- **Stage 2 (expander)** — lives in `patches-dsl`. Takes a parsed `File` AST
  and returns a `FlatPatch`: a template-free, fully concrete list of module
  instances and connections. Templates are inlined with namespaced node IDs,
  parameters are substituted, and cable scales are composed at template
  boundaries.

- **Stage 3 (graph builder)** — lives in `patches-interpreter`. Takes a
  `FlatPatch`, a `ModuleRegistry` (type name → factory), and an
  `AudioEnvironment`, and produces a `ModuleGraph` ready for the planner.
  Module type resolution, port validation, and parameter conversion happen
  here.

The result is a path from a `.patches` source file to a `ModuleGraph` without
touching any audio-backend code, keeping `patches-core` and `patches-dsl`
free of backend dependencies.

## Tickets

| ID   | Title                                  | Priority | Depends on   |
|------|----------------------------------------|----------|--------------|
| 0140 | Define `FlatPatch` IR in `patches-dsl` | high     | —            |
| 0141 | Template expander in `patches-dsl`     | high     | 0140         |
| 0143 | `FlatPatch`-to-`ModuleGraph` builder   | high     | 0140, 0141   |
| 0144 | Interpreter integration tests          | medium   | 0143         |

## Definition of done

- `patches-dsl` exports a `FlatPatch` type and an `expand(file: &File) ->
  Result<FlatPatch, ExpandError>` function; all template constructs are
  inlined; scales are composed correctly at boundaries.
- `patches-interpreter` exports a `build(flat: &FlatPatch, registry: &Registry,
  env: &AudioEnvironment) -> Result<ModuleGraph, InterpretError>` function;
  errors carry source spans. (`Registry` is `patches_core::Registry`.)
- Integration tests exercise the full pipeline (parse → expand → build) using
  the existing `.patches` fixture corpus and `patches_modules::default_registry()`.
- `cargo test` and `cargo clippy` pass with no warnings across all crates.
