---
id: "0264"
title: Tree-sitter grammar for patches DSL
priority: high
created: 2026-04-07
---

## Summary

Write a tree-sitter grammar that accepts the same language as the pest grammar
in `patches-dsl/src/grammar.pest`. Integrate the generated parser into the
`patches-lsp` crate.

## Acceptance criteria

- [ ] `patches-lsp/tree-sitter-patches/grammar.js` — tree-sitter grammar
      covering all pest grammar rules: file structure (`enum`, `template`,
      `patch`), module declarations with shape and param blocks, connections
      with arrows (forward, backward, scaled), port references with indices
      (literal, alias, arity), scalars (int, float, bool, string, note literal,
      unit literal, param ref, enum ref), arrays, tables, template port
      declarations (`in:`, `out:`), template param declarations, and at-blocks.
- [ ] Generated C parser compiles and is linked into `patches-lsp` via
      `tree-sitter` Rust bindings (build script in `build.rs` or
      `cc` crate).
- [ ] All valid `.patches` files in `examples/` and
      `patches-dsl/tests/fixtures/` (excluding `fixtures/errors/`) parse
      with no ERROR or MISSING nodes in the CST.
- [ ] Whitespace and comments (`#` to end of line) are handled as tree-sitter
      `extras`.
- [ ] Error recovery produces useful partial trees for common incomplete-input
      cases: unterminated module declaration, missing arrow in connection,
      unclosed brace/paren.
- [ ] `cargo clippy -p patches-lsp` passes clean.

## Notes

- The grammar should be a mechanical translation from the pest rules. Key
  mapping differences: pest `WHITESPACE`/`COMMENT` → tree-sitter `extras`;
  pest atomic rules → tree-sitter `token()`; pest ordered choice → tree-sitter
  `choice()`.
- Error recovery tuning is best done iteratively — get the happy path working
  first, then inspect CSTs for malformed input and add `prec()` / recovery
  hints.
- Epic: E048
