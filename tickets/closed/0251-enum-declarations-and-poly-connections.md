---
id: "0251"
title: Enum declarations and poly connection coverage
priority: medium
created: 2026-04-02
---

## Summary

Two DSL features are exported from the crate's public API but have no test
coverage in `patches-dsl`:

1. **Enum declarations** — `EnumDecl` is part of the AST and exported from
   `lib.rs`, but no test defines or references an enum.
2. **Poly connections** — T-0119 added a `poly` field to DSL cable connections,
   but no test in this crate asserts on the poly field of a `FlatConnection`.

## Acceptance criteria

- [ ] **Enum parsing:** A test that parses an `enum` declaration and verifies
      the AST contains an `EnumDecl` with the correct name and member list.
- [ ] **Enum member reference:** A test that uses `<enum>.<member>` in a scalar
      position (e.g. as a parameter value) and verifies it resolves to the
      correct integer index after expansion.
- [ ] **Enum error case:** A test referencing a non-existent enum member
      produces an ExpandError.
- [ ] **Poly connection:** A test that parses and expands a connection with the
      `poly` modifier and verifies the `FlatConnection` has the correct poly
      field value.
- [ ] **Poly default:** A test verifying that connections without the poly
      modifier have the default poly field value.

## Notes

- If enum declarations are not yet implemented in the expander (only parsed),
  then the enum expansion tests should be marked as documenting the gap, with
  a note on what T-number or epic will implement them.
- The poly syntax may be `~>` or a modifier — check the grammar and T-0119
  for the actual surface syntax.
- Epic: E046
