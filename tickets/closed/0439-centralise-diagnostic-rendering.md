---
id: "0439"
title: Centralise per-consumer diagnostic rendering
priority: high
created: 2026-04-15
---

## Summary

Three consumers currently reimplement error → diagnostic conversion:

- Player (`patches-player/src/diagnostic_render.rs`) uses ariadne on top
  of `patches_diagnostics::RenderedDiagnostic` — this is the reference
  implementation.
- CLAP (`patches-clap/src/plugin.rs:195-298`) hand-rolls
  `compile_error_to_diagnostics`, `synthetic_diagnostic`,
  `span_diagnostic`, `interpret_diagnostic`, `plan_diagnostic`.
- LSP (`patches-lsp/src/workspace.rs:851-908`) hand-constructs
  `RenderedDiagnostic` from error variants inline while bucketing per
  URI.

All three must produce identical diagnostics for identical source.
Consolidate the conversion logic in `patches-diagnostics` so each
consumer only differs in presentation (terminal vs. LSP diagnostic vs.
plugin status panel).

## Acceptance criteria

- [ ] `patches-diagnostics` owns conversion from every pipeline error
      type (`LoadError`, `ExpandError`, `BindErrorCode`, `InterpretError`,
      expand `Warning`) to `RenderedDiagnostic`.
- [ ] Shared `render_provenance_error(code, message, prov, label)`
      builder eliminates converter-method repetition in
      `patches-diagnostics/src/lib.rs:53-230`.
- [ ] CLAP plugin `compile_error_to_diagnostics` and helpers deleted;
      plugin consumes `RenderedDiagnostic` directly.
- [ ] LSP `render_pipeline_diagnostics` delegates to the shared
      converters, keeping only the LSP-specific URI bucketing and
      line-index mapping.
- [ ] Round-trip test: crafted pipeline errors produce the same
      `RenderedDiagnostic` values whether invoked from player, CLAP, or
      LSP call paths.
- [ ] `cargo test -p patches-diagnostics`, `cargo clippy` clean.

## Notes

Part of E082. Should land before 0440 so the warning-emission site has
one converter path to feed.
