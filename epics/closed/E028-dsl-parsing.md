---
id: "E028"
title: DSL parsing — Stage 1
created: 2026-03-19
tickets: ["0138", "0139"]
---

## Summary

The Patches DSL (specified in ADR 0005 and ADR 0006) is the authoring format
for patches. It replaces the interim YAML format once the full pipeline is in
place. This epic covers Stage 1 of the three-stage compilation pipeline: the
PEG parser and the AST it produces.

The approach is test-driven: a corpus of hand-authored `.patches` files is
written first, covering all syntax constructs, and the grammar is then
developed to parse every file in that corpus successfully. Negative-case
fixtures assert that malformed input produces parse errors rather than
silently misparsing.

The expander (Stage 2) and the graph builder / interpreter (Stage 3) are out
of scope for this epic and will follow once the parser is stable.

## Tickets

| ID   | Title                              | Priority | Depends on |
|------|------------------------------------|----------|------------|
| 0138 | DSL example patch corpus           | high     | —          |
| 0139 | PEG grammar and parser             | high     | 0138       |

## Definition of done

- A corpus of `.patches` fixture files lives in `patches-dsl/tests/fixtures/`,
  covering: flat patches, scaled connections, indexed ports, array/table params,
  single templates, and nested templates.
- A companion set of negative-case fixtures lives in
  `patches-dsl/tests/fixtures/errors/`, each containing exactly one syntax
  error.
- `patches-dsl` contains a `pest` PEG grammar, AST types with source spans,
  and a public `parse(src: &str) -> Result<File, ParseError>` entry point.
- Parser tests assert `Ok` for every positive fixture and `Err` for every
  negative fixture.
- `cargo test -p patches-dsl` passes with no warnings and no `clippy`
  complaints.
