---
id: "0139"
title: PEG grammar and parser
priority: high
created: 2026-03-19
---

## Summary

Implement Stage 1 of the DSL compilation pipeline in `patches-dsl`: a `pest`
PEG grammar, an AST that preserves source spans, and a public parse entry
point. The corpus from T-0138 is the test suite: every positive fixture must
parse to `Ok`, every negative fixture must produce `Err`.

## Acceptance criteria

- [ ] `patches-dsl/Cargo.toml` adds `pest` and `pest_derive` as dependencies
  (confirm with the user before adding).

- [ ] `patches-dsl/src/grammar.pest` implements the full grammar from ADR 0006,
  covering:
  - Lexical rules: whitespace, line comments, identifiers, integer and
    floating-point literals, boolean literals, double-quoted strings.
  - `Value` hierarchy: `Scalar`, `Array`, `Table`.
  - `ShapeBlock` and `ParamBlock`.
  - `ModuleDecl`.
  - `PortRef` with optional `Index`.
  - `Connection` with `ForwardArrow` (`->`, `-[N]->`) and `BackwardArrow`
    (`<-`, `<-[N]-`).
  - `PortDecls` (`in:` / `out:` lines).
  - `Template` with optional `ParamDecls`.
  - `Patch` block.
  - `File` root rule.

- [ ] `patches-dsl/src/ast.rs` defines Rust types that mirror the grammar
  structure. Each node carries a `Span` (byte-offset range into the source
  string) for use in later error reporting. Key types:

  - `File { templates: Vec<Template>, patch: Patch }`
  - `Template { name, params: Vec<ParamDecl>, in_ports, out_ports, body: Vec<Statement> }`
  - `ParamDecl { name, ty: ParamType, default: Option<Scalar> }`
  - `Patch { body: Vec<Statement> }`
  - `Statement` — either `ModuleDecl` or `Connection`
  - `ModuleDecl { name, type_name, shape: Vec<ShapeArg>, params: Vec<ParamEntry> }`
  - `Connection { lhs: PortRef, arrow: Arrow, rhs: PortRef }`
  - `Arrow { direction: Direction, scale: Option<f64> }`
  - `PortRef { module: Ident, port: Ident, index: Option<u32> }`
  - `Value` — `Scalar(Scalar)`, `Array(Vec<Value>)`, `Table(Vec<(Ident, Value)>)`
  - `Scalar` — `Int(i64)`, `Float(f64)`, `Bool(bool)`, `Str(String)`,
    `Ident(String)` (template param reference)

- [ ] `patches-dsl/src/parser.rs` exposes:
  ```rust
  pub fn parse(src: &str) -> Result<File, ParseError>
  ```
  `ParseError` carries the source span and a human-readable message.

- [ ] `patches-dsl/src/lib.rs` re-exports `parse`, `File`, `ParseError`, and
  all AST types.

- [ ] `patches-dsl/tests/parser_tests.rs` contains:
  - One `#[test]` per positive fixture in `tests/fixtures/`, asserting
    `parse(...).is_ok()`.
  - One `#[test]` per negative fixture in `tests/fixtures/errors/`, asserting
    `parse(...).is_err()`.

- [ ] `cargo test -p patches-dsl` passes — all fixture tests green.
- [ ] `cargo clippy -p patches-dsl` produces no warnings.
- [ ] No `unwrap()` or `expect()` in library code.

## Notes

`pest` is the parser generator specified in ADR 0005. The grammar file is
`src/grammar.pest`; `pest_derive` generates the `Rule` enum via a derive macro
on a unit struct.

The AST at this stage is a faithful representation of surface syntax only —
no name resolution, no semantic validation, no template expansion. The
expander (Stage 2) will be a separate crate concern layered on top of this
AST.

Spans should use byte offsets (`usize`) into the original source string, not
line/column pairs; line/column can be computed on demand for error display
without storing it in every node.

Depends on T-0138 (fixture corpus must exist before parser tests can be written).
