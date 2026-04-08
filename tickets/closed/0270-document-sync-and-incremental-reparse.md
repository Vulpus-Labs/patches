---
id: "0270"
title: Document sync and incremental reparse
priority: high
created: 2026-04-07
---

## Summary

Wire document lifecycle events (`didOpen`, `didChange`, `didClose`) into the
LSP server. On each change, incrementally reparse using tree-sitter's edit API,
rebuild the tolerant AST, and re-run semantic analysis. Maintain a stable
semantic model from the last successful analysis.

## Acceptance criteria

- [ ] The server tracks open documents with their current source text and
      tree-sitter `Tree`.
- [ ] `didOpen` triggers a full parse and analysis.
- [ ] `didChange` applies tree-sitter edits incrementally and reparses. The
      tolerant AST and semantic model are rebuilt from the new tree.
- [ ] The server maintains a "stable model" — the semantic model from the last
      analysis that produced at least a partial result. This is used for
      completions even when the current file state has errors.
- [ ] `didClose` cleans up document state.
- [ ] Tests verify that incremental reparse produces the same tree as a full
      reparse after the same edits.

## Notes

- tree-sitter's `Tree::edit` + `Parser::parse` with the old tree enables
  incremental reparsing. The LSP receives full document content with
  `TextDocumentSyncKind::Full` initially; switching to incremental sync is a
  future optimisation.
- Even with full sync, calling `Tree::edit` before `Parser::parse` lets
  tree-sitter reuse unchanged subtrees internally.
- Epic: E050
