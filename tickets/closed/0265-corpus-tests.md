---
id: "0265"
title: Corpus tests from existing fixture and example files
priority: high
created: 2026-04-07
---

## Summary

Generate tree-sitter corpus test entries from the existing `.patches` files.
These tests verify that the tree-sitter grammar agrees with the pest grammar on
all valid input and serve as regression tests going forward.

## Acceptance criteria

- [ ] `patches-lsp/tree-sitter-patches/corpus/` contains corpus test files
      in tree-sitter's standard format (input/expected S-expression pairs).
- [ ] Corpus entries cover at least: simple flat patch, module with shape args,
      module with param block, scaled connections (forward and backward),
      indexed ports (literal, alias, arity), templates with port and param
      declarations, nested templates, enum declarations, unit literals (Hz, dB),
      note literals, array params, table params, at-blocks, and param refs.
- [ ] `tree-sitter test` (or equivalent Rust test) passes for all corpus
      entries.
- [ ] At least 3 corpus entries for malformed input verifying that ERROR/MISSING
      nodes appear in expected positions (unclosed brace, missing arrow,
      incomplete module declaration).

## Notes

- Corpus entries can be generated semi-automatically: parse each fixture with
  tree-sitter, dump the S-expression, write the pair. Manual review is needed
  to ensure the S-expressions are sensible.
- Epic: E048
