---
id: "0408"
title: Update fixtures, manual, and LSP for new song syntax
priority: medium
created: 2026-04-14
---

## Summary

Update `patches-lsp` (hover, go-to-definition, diagnostics) to
understand the new `section` and `play` constructs, and refresh the
manual (`docs/src/`) with the new syntax. Add golden fixtures
exercising the full surface.

## Acceptance criteria

- [ ] LSP go-to-definition works on `section` name references inside
      `play` expressions, including jumping to a named-inline
      definition site.
- [ ] LSP hover shows section signature (expected lane width, row
      count).
- [ ] Manual pages describing `song`, `section`, `play`, `@loop`, row
      groups, and inline definitions added/updated; ADR 0029 song
      examples revised to match.
- [ ] New `.patches` fixtures cover: nested row groups, play
      composition (`,`, `*`, groups), anonymous and named-inline play
      bodies, `@loop`, inline patterns, top-level sections.

## Notes

Depends on 0407.
