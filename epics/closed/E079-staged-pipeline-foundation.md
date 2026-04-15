---
id: "E079"
title: Staged patch loading pipeline — foundation
created: 2026-04-15
status: open
depends_on: ["ADR-0038"]
tickets: ["0426", "0427", "0428", "0429"]
---

## Summary

Implements the stage separation from ADR 0038 without changing consumer
behaviour. Splits structural checks out of `patches-dsl::expand` into a
distinct post-expansion pass, narrows `patches-interpreter::build` to
registry binding only, introduces a pipeline orchestrator with named
stage entry points, and extends `patches-diagnostics` so every stage's
error type renders uniformly. Player, CLAP, and LSP keep calling their
existing entry points; the orchestrator wraps the new shape so the
consumer migration epic (E080) can swap them in behind a small,
testable surface.

## Acceptance criteria

- [ ] `patches-dsl::expand` emits mechanical expansion only; structural
      errors (unknown param/alias/module, missing or multiple `patch`
      blocks, recursive template instantiation) come from a new pass.
- [ ] `patches-interpreter::InterpretError` covers registry binding
      only; structural cases have been moved to stage 3a's error type.
- [ ] A pipeline orchestrator exposes stage entry points with clear
      input/output types for each stage boundary.
- [ ] `patches-diagnostics` renders `LoadError`, `StructuralError`, and
      `InterpretError` with consistent severity/code/snippet schema.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean across the
      workspace. Existing consumers still compile and pass tests.

## Tickets

| ID   | Title                                                |
|------|------------------------------------------------------|
| 0426 | Extract structural-checks pass from DSL expander     |
| 0427 | Narrow InterpretError to registry binding only       |
| 0428 | Pipeline orchestrator with staged entry points       |
| 0429 | Diagnostic converters for every pipeline stage       |

## Notes

This epic is deliberately refactor-only. No consumer observable
behaviour should change. E080 then rewires player, CLAP, and LSP onto
the staged entry points and introduces the fail-fast vs aggregate
policy split; E078's deferred LSP features become unblocked once E080
lands.
