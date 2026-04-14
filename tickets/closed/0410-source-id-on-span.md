---
id: "0410"
title: Add SourceId to Span and introduce SourceMap
priority: medium
created: 2026-04-14
epic: E075
---

## Summary

`Span` in `patches-dsl/src/ast.rs` is currently `{ start: usize, end: usize }`
— raw byte offsets with no file identity. Once the loader merges
included files, a span's file of origin is no longer recoverable.
Introduce `SourceId(u32)` on `Span` and a `SourceMap` that owns file
paths and source strings.

This phase is a standalone mechanical refactor. No behavioural change.

## Acceptance criteria

- [ ] `SourceId(pub u32)` defined; `Span` gains `source: SourceId`.
- [ ] New `patches-dsl/src/source_map.rs` owns
      `Vec<(PathBuf, String)>` and assigns IDs monotonically.
- [ ] `SourceId(0)` reserved as synthetic sentinel; documented.
- [ ] Parser (`patches-dsl/src/parser.rs`) takes a `SourceId` and
      threads it into every produced `Span`.
- [ ] Loader (`patches-dsl/src/loader.rs`) assigns IDs per file
      parsed and returns the `SourceMap` alongside the AST.
- [ ] All `Span { start, end }` literals in tests and `ast_builder.rs`
      updated.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean across workspace.

## Notes

- Line/column conversion happens at render time against
  `SourceMap.source_text(id)`; no need to precompute.
- LSP's existing span→line-col conversion
  (`patches-lsp/src/lsp_util.rs`) will need to consult the SourceMap
  rather than its cached single buffer. In-scope for this ticket.
- `loader.rs` already carries `Vec<(PathBuf, Span)>` in
  `LoadError::include_chain`; keep that untouched for now — it's
  orthogonal and will be revisited (or not) when chains are rendered
  in 0414.

## Risks

- Test fixtures may have many `Span` literals. A constructor
  `Span::new(source, start, end)` plus a `Span::synthetic()` helper
  will keep the churn localised.
