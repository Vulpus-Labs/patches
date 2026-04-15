---
id: "0451"
title: Hoist ParamIndex/PortIndex Arity variants out of parser AST
priority: medium
created: 2026-04-15
---

## Summary

`ast.rs` defines `ParamIndex` and `PortIndex` with `Literal`, `Alias`,
and `Arity` variants (ast.rs:65–72, 125–132). The `Arity` variant exists
only so the expander can defer resolution — it encodes a semantic
distinction that belongs downstream of the parser. The parser should
emit a simpler optional-string index type and let the expander classify
it.

## Acceptance criteria

- [ ] Parser emits a single unified index type (e.g. `Option<String>`
      or a two-variant `IndexToken`) without the `Arity` discrimination.
- [ ] Expander classifies tokens as literal / alias / arity at use
      sites.
- [ ] Grammar tests unchanged; expander tests cover arity classification.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

E083. Keeps the AST a faithful syntactic mirror and moves semantic
interpretation into the one stage that needs it.
