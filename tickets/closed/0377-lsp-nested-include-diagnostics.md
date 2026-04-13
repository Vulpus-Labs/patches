---
id: "0377"
title: Surface nested include diagnostics in LSP
priority: medium
created: 2026-04-13
---

## Summary

In `patches-lsp/src/server.rs` line 180, diagnostics from recursively-resolved
includes are silently discarded (`let _nested_diags = ...`). If an included
file's own includes are broken, the user gets no feedback.

## Acceptance criteria

- [ ] Nested include diagnostics are collected and published to the parent document
- [ ] Diagnostics from transitive includes indicate the include chain (e.g. "in file included from ...")
- [ ] Test: file A includes B, B includes missing C — diagnostic appears on A's include of B

## Notes

Consider whether diagnostics should appear on the immediate `include` directive
in the parent, or as a separate diagnostic set. The former is simpler and
matches how C compilers report nested `#include` errors.
