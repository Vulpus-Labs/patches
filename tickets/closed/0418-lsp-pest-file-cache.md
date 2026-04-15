---
id: "0418"
title: LSP pest File cache and expansion integration
priority: medium
created: 2026-04-14
---

## Summary

Wire `patches-dsl`'s pest parser and `expand()` into `patches-lsp` alongside the existing tree-sitter tolerant pipeline. Cache pest `File` per URL so editing one file does not re-parse unchanged includes from disk. Cache `FlatPatch` + span→FlatNode reverse index per root, invalidated via the existing `included_by` reverse graph.

This is the foundation for expansion-aware features (hover, diagnostics inside template bodies, peek expansion). See ticket 0419 for the hover handler.

## Acceptance criteria

- [ ] `DocumentState` gains `pest_file: Option<File>` — `Some` when tree-sitter reports a clean parse, `None` otherwise
- [ ] `WorkspaceState` gains `flat_cache: HashMap<Url, FlatPatch>` and `span_index: HashMap<Url, SpanIndex>` keyed by root URL
- [ ] On doc change: tree-sitter reparse (existing); if clean, also pest-reparse that file; invalidate `flat_cache`/`span_index` for the file and all transitive ancestors via `included_by`
- [ ] Re-expansion driven lazily from feature handlers (hover/diagnostics), not on every keystroke
- [ ] Loader invoked with in-memory closure: returns source from `DocumentState` when available, disk read otherwise; unopened includes cached in a loader-side map for the call
- [ ] Edge case: file transitions clean → broken → clean, `pest_file` correctly rebuilt; downstream `flat_cache` entries invalidated on every transition
- [ ] Tests: swap-in behaviour (edit one file, assert others not reparsed), broken-syntax file leaves neighbours' caches intact, cascade invalidation on include-graph ancestor edit
- [ ] `cargo clippy` and `cargo test` clean

## Notes

Why pest-alongside-tree-sitter: converting the tolerant tree-sitter AST into a pest `File` is heavy (option A) and making `expand()` generic over AST is invasive (option B). Running both parsers with pest gated on clean parse (option C) is simpler and matches the tolerant/deep-analysis split we already have for diagnostics.

Three cache layers, different lifetimes:

| Layer | Invalidate on |
|-------|---------------|
| Source text (per URL) | edit of that file |
| Tree-sitter tree (per URL) | edit of that file (incremental reparse) |
| Pest `File` (per URL) | edit of that file |
| `FlatPatch` + span index (per root) | edit of any file in root's include closure |

Existing infra to reuse:

- `patches-dsl::loader::LoadResult` accepts a file-reading closure — tested with in-memory maps (`patches-dsl/src/loader.rs`)
- `patches-dsl::parser::parse_with_source` takes a string, returns `File` (`patches-dsl/src/parser.rs:203`)
- `WorkspaceState.included_by` / `includes_of` already maintain the reverse graph (`patches-lsp/src/workspace.rs:52-58`)
- `FlatModule.span` and `FlatConnection.span` provide forward provenance (`patches-dsl/src/flat.rs`). Reverse index is new but mechanical.

Open sub-question (decide during implementation, do not block this ticket):

- Loader re-parses every file per `load` call. For big include trees on edit cascade this re-parses unchanged files. Simple path: keep loader API, rely on in-memory source + cheap pest parse. Optimised path: extend loader to accept pre-parsed `File` for unchanged files. Start with simple; measure before extending.

Degradation: when a file has syntax errors, its `pest_file` is `None`; any root whose include closure contains that file cannot be expanded; feature handlers for those roots fall back to tolerant-only behaviour. Tolerant diagnostics continue to work regardless.
