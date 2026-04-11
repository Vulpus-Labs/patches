---
id: "0314"
title: "tree-sitter grammar: pattern and song block rules"
priority: medium
created: 2026-04-11
---

## Summary

Extend the tree-sitter grammar in `patches-lsp/tree-sitter-patches/` to
recognise `pattern` and `song` blocks, enabling syntax highlighting and
tolerant parsing in the VS Code extension.

## Acceptance criteria

- [ ] `pattern_block` rule in `grammar.js`: keyword, name, braces,
      channel rows with label + colon + steps
- [ ] Step token rules covering: note literals, `x`, `.`, `~`, float
      literals, unit suffixes, cv2 (`:` separator), slides (`>`),
      repeats (`*n`), `slide()` generator
- [ ] Line continuation (`|` at end of channel row) handled
- [ ] `song_block` rule in `grammar.js`: keyword, name, braces,
      pipe-delimited table rows, `@loop` annotation
- [ ] Top-level `source_file` rule updated to accept pattern and song
      blocks
- [ ] `src/parser.c` regenerated from updated grammar
- [ ] Tree-sitter test corpus covers: pattern with various step types,
      song with header/rows/loop, mixed file with templates + patterns +
      songs + patch
- [ ] Syntax highlighting queries updated for new node types

## Notes

The tree-sitter grammar is intentionally more tolerant than the pest
grammar — it should parse partial/invalid input gracefully for editor
support. Error recovery is more important than strict validation here;
validation is the LSP diagnostics layer's job (ticket 0317).

Epic: E058
ADR: 0029
