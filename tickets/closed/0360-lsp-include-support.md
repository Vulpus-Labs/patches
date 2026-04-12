---
id: "0360"
title: LSP include-aware analysis and navigation
priority: medium
created: 2026-04-12
epic: "E067"
depends: ["0357"]
---

## Summary

Extend the LSP to parse `include` directives, resolve included files, analyse them, and feed their definitions into the existing `NavigationIndex` for cross-file go-to-definition.

## Design

The LSP already has multi-file navigation groundwork:

- `NavigationIndex` stores `(Url, Span)` per definition and resolves cross-file references.
- `goto_definition` returns `(Url, Span)` tuples.
- A `cross_file_resolution` test validates the architecture.
- The server's goto-definition handler has a placeholder for cross-file span conversion.

**Tree-sitter grammar:**

Add `include_directive` as a named node in the tree-sitter grammar (same syntax: `include "path"`).

**Include resolution in the server** (`patches-lsp/src/server.rs`):

When a document is opened or changed:

1. Walk the tree-sitter CST for `include_directive` nodes.
2. Extract the path string from each directive.
3. Resolve relative to the document's URI directory.
4. For each resolved path not already in the `documents` map:
   - Read the file from disk.
   - Parse with tree-sitter.
   - Build `SemanticModel` + `FileNavigation`.
   - Insert into `documents` as a "background" document (not editor-opened).
5. Call `nav_index.rebuild()` as usual — it already processes all entries in `documents`.

**Cross-file goto-definition:**

Already works. The server's span conversion code (`server.rs:239-246`) looks up the target file's `line_index` in `docs`. Since included files are now in `docs`, their line indices are available.

**Diagnostics for includes:**

- Missing include file: publish a diagnostic on the `include` directive's range.
- Cycle detected: diagnostic on the directive that closes the cycle.

## Acceptance criteria

- [ ] Tree-sitter grammar recognises `include "path"` directives
- [ ] Server resolves includes and loads referenced files into document map
- [ ] Definitions from included files appear in `NavigationIndex`
- [ ] Go-to-definition from master file jumps to definition in included file
- [ ] Go-to-definition from included file jumps to definition in another included file
- [ ] Missing include file produces a diagnostic on the directive
- [ ] Include cycle produces a diagnostic
- [ ] Editing an included file (open in editor) updates the index via existing `did_change`
- [ ] `cargo test -p patches-lsp` and `cargo clippy` pass

## Notes

- Hot-reload of included files that are not open in the editor is deferred. The user can reopen or edit the master file to pick up external changes.
- Background documents (loaded from includes) should be removed from `documents` when the including file is closed or when the include directive is removed. Track which documents are include-loaded vs editor-opened to manage this lifecycle.
