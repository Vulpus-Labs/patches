---
id: "0738"
title: DSL shape_block collapse to single positional arg
priority: medium
created: 2026-04-28
epic: "E126"
adrs: ["0060"]
depends_on: ["0735"]
---

## Summary

Collapse the DSL `shape_block` to a single positional arg, either a
scalar (`int` or `<param_ref>`) or an alias list. Delete the
`shape_arg` rule and the `channels:` named-key form. Migrate examples,
update the LSP, and refresh docs.

## Acceptance criteria

- [ ] `patches-dsl/src/grammar.pest`: `shape_block` becomes
      `"(" ~ (scalar | alias_list)? ~ ")"`. `shape_arg` rule deleted.
- [ ] AST update in `patches-dsl/src/ast.rs`: `ShapeBlock` carries an
      `Option<ShapeValue>` where `ShapeValue` is `Scalar | AliasList`.
- [ ] Parser, expander, and validator updated. Template-passthrough
      `Foo(<channels>)` works uniformly whether the bound value is an
      int or an alias list.
- [ ] Migrate every `.patches` file under `examples/`,
      `patches-dsl/tests/fixtures/`,
      `patches-integration-tests/tests/fixtures/`, and any inline
      DSL strings in tests:
      - `Foo(channels: 8)` → `Foo(8)`
      - `Foo(channels: [a, b, c])` → `Foo([a, b, c])`
      - `Foo(channels: <n>)` → `Foo(<n>)`
- [ ] `patches-lsp` completions and hover updated; the
      `channels:` named-key form no longer offered.
- [ ] `examples/CLAUDE.md` updated to reflect the new syntax.
- [ ] `cargo run -q -p patches-tools --bin patches-check --
      examples/*.patches` passes for every example.
- [ ] `cargo test` passes.

## Notes

Variable-arity templates (ADR 0019) are unchanged. Templates whose
`int` params bind alias lists already work — alias lists evaluate to
ints with name provenance.

Provide a one-shot migration script (Python or shell) that does the
mechanical rewrite across the tree, since the change is purely
syntactic.
