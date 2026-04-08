---
id: "E027"
title: "<param> syntax and unquoted string literals"
status: closed
priority: high
created: 2026-03-20
tickets:
  - "0160"
  - "0161"
  - "0162"
  - "0163"
---

## Summary

Introduces `<param>` as the universal template parameter reference syntax,
replacing the current bare-identifier convention. Bare identifiers in value
positions become unquoted string literals, allowing `waveform: sine` and
`fm_type: log` without quotes. A shorthand form `<param>` inside a param block
expands to `param: <param>`. Structural interpolation — `<param>` in a port
label or arrow scale — is also added.

See ADR 0006 amendment (2026-03-20) for the full design.

## Tickets

- [T-0160](../tickets/open/0160-param-ref-ast.md) — AST: `Scalar::ParamRef`, unquoted string literal, `PortLabel`, `Arrow::scale` as `Scalar`
- [T-0161](../tickets/open/0161-param-ref-parser.md) — Parser: `<ident>` syntax, bare ident as string, shorthand entries, port label and scale interpolation
- [T-0162](../tickets/open/0162-param-ref-expander.md) — Expander: resolve `ParamRef` in all positions, expand shorthand entries
- [T-0163](../tickets/open/0163-param-ref-tests.md) — Tests: comprehensive coverage of new syntax forms
