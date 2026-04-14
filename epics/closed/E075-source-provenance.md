---
id: "E075"
title: Source provenance through template expansion
created: 2026-04-14
tickets: ["0410", "0411", "0412", "0413", "0414"]
---

## Summary

Thread source provenance through the DSL → FlatPatch → interpreter →
BuildError pipeline so that diagnostics can point at the author's code
even when a failure originates deep inside template expansion.

See ADR 0036 for rationale and design.

The work is split into five phases, each leaving the tree compilable
and tested.

## Acceptance criteria

- [ ] `Span` carries a `SourceId`; a `SourceMap` tracks file identity
      across loads.
- [ ] FlatPatch nodes carry `Provenance` (call-site chain +
      definition site) in place of single-span.
- [ ] Both `BuildError` enums carry `origin: Option<Provenance>`.
- [ ] `patches-player` renders expansion chains in error output.
- [ ] `patches-lsp` surfaces expansion chains via
      `DiagnosticRelatedInformation`.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Tickets

| ID   | Title                                                  |
|------|--------------------------------------------------------|
| 0410 | Add SourceId to Span and introduce SourceMap           |
| 0411 | Introduce Provenance and thread through expand.rs      |
| 0412 | Migrate FlatPatch nodes from Span to Provenance        |
| 0413 | Add origin to BuildError enums                         |
| 0414 | Render expansion chains in player, LSP, and CLAP host  |
