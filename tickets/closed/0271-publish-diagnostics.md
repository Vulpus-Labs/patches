---
id: "0271"
title: Publish diagnostics on document change
priority: high
created: 2026-04-07
---

## Summary

After each analysis pass, convert accumulated diagnostics to LSP `Diagnostic`
objects and publish them to the editor via `textDocument/publishDiagnostics`.

## Acceptance criteria

- [ ] Diagnostics from the CST builder (syntax errors) and semantic analysis
      (unknown types, invalid params, bad ports) are mapped to LSP `Diagnostic`
      with appropriate severity (`Error` for syntax and unknown types,
      `Warning` for less critical issues).
- [ ] Byte-offset spans are converted to LSP `Position` (line/character) using
      a line index built from the source text.
- [ ] Diagnostics are published on every `didOpen` and `didChange`.
- [ ] When a file is corrected, stale diagnostics are cleared (empty
      diagnostics array published).
- [ ] Manual verification: introduce a typo in a module type name in VS Code,
      see a red underline; fix it, underline disappears.

## Notes

- Epic: E050
