---
id: "0160"
title: "AST: ParamRef, unquoted string literal, PortLabel, Arrow scale as Scalar"
priority: high
created: 2026-03-20
epic: E027
---

## Summary

Update the `patches-dsl` AST to represent the new `<param>` reference syntax
and unquoted string literals. This is a pure data-model change; the parser and
expander are updated in subsequent tickets.

## Acceptance criteria

- [ ] `Scalar` gains a `ParamRef(String)` variant. The existing `Ident(String)`
  variant is renamed to `Str(String)` and its semantics change from "template
  parameter reference" to "unquoted string literal". All existing uses of
  `Scalar::Ident` in the codebase are updated to `Scalar::Str`.
- [ ] A `PortLabel` type is introduced:
  ```rust
  pub enum PortLabel {
      Literal(String),   // a concrete port name
      Param(String),     // <ident> — resolved to a string at expansion time
  }
  ```
  `PortRef::port` changes type from `Ident` to `PortLabel`.
- [ ] `Arrow::scale` changes type from `Option<f64>` to `Option<Scalar>`. Only
  `Scalar::Float`, `Scalar::Int`, and `Scalar::ParamRef` are meaningful here;
  other variants are rejected at a later stage.
- [ ] A `ParamEntry` shorthand variant is introduced in the AST to represent
  `<ident>` alone inside a param block (without an explicit key). This may be
  represented as a new `ParamEntry::Shorthand(String)` variant or by reusing
  `ParamEntry` with a flag — choose whichever is cleaner.
- [ ] All types continue to derive `Debug` and `Clone`.
- [ ] `cargo clippy -p patches-dsl` passes with no warnings.
- [ ] `cargo test -p patches-dsl` passes (existing tests may need updating for
  renamed variants; no new behaviour is introduced in this ticket).

## Notes

The rename of `Scalar::Ident` → `Scalar::Str` is the most disruptive part.
Use `replace_all` in the editor; grep for `Scalar::Ident` to find all call
sites including tests and the expander's `subst_scalar`.

`Arrow::scale: Option<Scalar>` means the expander (T-0162) must resolve it to
a concrete float at expansion time. The graph builder (Stage 3) already
receives a concrete `FlatConnection::scale: f64`, so `FlatConnection` is
unchanged.
