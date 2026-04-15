---
id: "0429"
title: Diagnostic converters for every pipeline stage
priority: medium
created: 2026-04-15
---

## Summary

`patches-diagnostics` currently renders `BuildError` and `ExpandError`
but not `LoadError` or `InterpretError`, and after 0426 there will be
a new `StructuralError` to cover as well. Add converters so every
pipeline-stage error type maps to a `RenderedDiagnostic` with
consistent severity, error code scheme, and snippet formatting.

## Acceptance criteria

- [ ] Converter from `LoadError` (include cycles, IO, UTF-8, name
      collisions) → `RenderedDiagnostic`.
- [ ] Converter from `StructuralError` → `RenderedDiagnostic`.
- [ ] Converter from `InterpretError` → `RenderedDiagnostic` with
      provenance chain rendered in snippets where available.
- [ ] Error code prefix scheme documents stage ownership (e.g. `LD`
      load, `PA` parse, `EX` expand, `ST` structural, `BN` bind).
- [ ] Snapshot tests for each converter variant.
- [ ] `cargo test -p patches-diagnostics`, `cargo clippy` clean.

## Notes

Depends on 0426, 0427. The rendered diagnostics are what LSP publishes
and what player/CLAP print to console, so the schema has to be one
shape both can consume.
