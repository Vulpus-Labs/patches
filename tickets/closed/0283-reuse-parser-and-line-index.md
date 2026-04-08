---
id: "0283"
title: Reuse Parser instance across requests
priority: low
created: 2026-04-08
---

## Summary

A new `tree_sitter::Parser` is constructed on every call to
`analyse_and_publish`. Parsers are designed to be reused and support
incremental parsing via the `old_tree` parameter (currently passed as `None`).

## Acceptance criteria

- [ ] `PatchesLanguageServer` holds a `Mutex<Parser>` (or creates one per
      `analyse_and_publish` call but passes the previous `Tree` as `old_tree`
      for incremental parsing).
- [ ] `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.

## Notes

Incremental parsing requires calling `tree.edit()` before re-parsing, which
needs the old and new byte ranges from the LSP `contentChanges`. This is only
possible with `TextDocumentSyncKind::INCREMENTAL` — the server currently uses
`FULL` sync. If switching to incremental sync is too much scope for this ticket,
at minimum reuse the `Parser` instance.
