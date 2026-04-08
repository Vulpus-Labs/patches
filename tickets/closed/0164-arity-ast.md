---
id: "0164"
title: "AST: PortIndex enum, arity on port declarations, group params"
priority: high
created: 2026-03-20
epic: E028
depends_on: ["0160"]
---

## Summary

Extend the `patches-dsl` AST to represent variable-arity port groups and
parameter groups as described in ADR 0019.

## Acceptance criteria

- [ ] `PortIndex` is introduced as an enum replacing `Option<u32>` in `PortRef`:
  ```rust
  pub enum PortIndex {
      Literal(u32),   // port[0]
      Param(String),  // port[k]   — single port at computed index
      Arity(String),  // port[*n]  — expand over 0..n
  }
  ```
  `PortRef::index` changes from `Option<u32>` to `Option<PortIndex>` (absent
  = index 0 as before).
- [ ] Template port declarations gain an optional arity annotation. The
  existing `Template::in_ports: Vec<Ident>` and `out_ports: Vec<Ident>` become
  `Vec<PortGroupDecl>` where:
  ```rust
  pub struct PortGroupDecl {
      pub name: Ident,
      pub arity: Option<String>,  // Some("n") for  in: audio[n]
      pub span: Span,
  }
  ```
- [ ] `ParamDecl` gains an optional arity field:
  ```rust
  pub struct ParamDecl {
      pub name: Ident,
      pub arity: Option<String>,  // Some("size") for  level[size]: float
      pub ty: ParamType,
      pub default: Option<Scalar>,
      pub span: Span,
  }
  ```
- [ ] All new types derive `Debug` and `Clone`.
- [ ] `cargo clippy -p patches-dsl` passes with no warnings.
- [ ] `cargo test -p patches-dsl` passes (existing tests updated for changed
  field types; no new behaviour in this ticket).

## Notes

`PortIndex::Arity` only appears in connection port references (use sites), never
in `PortGroupDecl`. The declaration uses a plain param name; the `*` is a
use-site operator only.
