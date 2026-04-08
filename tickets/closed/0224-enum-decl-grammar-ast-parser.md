---
id: "0224"
title: Grammar, AST, and parser for enum_decl
priority: medium
created: 2026-03-30
---

## Summary

Add `enum` declarations to the DSL grammar and AST. An enum declares a named
set of identifiers whose values are their zero-based positions:

```text
enum drum {
    kick,
    snare,
    hat
}
```

This ticket covers parsing only. Enum references in scalar position (`drum.kick`)
are parsed but not yet expanded — that is done in T-0225.

## Acceptance criteria

- [ ] `grammar.pest` gains:
  - `enum_decl = { "enum" ~ ident ~ "{" ~ (ident ~ ","?)* ~ "}" }`
  - `file` rule updated to `SOI ~ enum_decl* ~ template* ~ patch ~ EOI`.
- [ ] `ast.rs` gains:
  - `pub struct EnumDecl { pub name: Ident, pub members: Vec<Ident>, pub span: Span }`
  - `File.enums: Vec<EnumDecl>` field added.
- [ ] `ast.rs` gains `Scalar::EnumRef { enum_name: String, member: String }` variant.
- [ ] `grammar.pest` gains `enum_ref = ${ ident ~ "." ~ ident }` and `scalar`
      updated to include `enum_ref` (ordered before `ident` to avoid partial
      consumption).
- [ ] `parser.rs` builds `EnumDecl` nodes and `Scalar::EnumRef` values correctly.
- [ ] Round-trip parse tests: an `enum` block produces the expected `EnumDecl`;
      `drum.kick` in scalar position produces `Scalar::EnumRef { "drum", "kick" }`.
- [ ] `cargo test` passes, `cargo clippy -- -D warnings` clean.

## Notes

Enum member names need not be globally unique at the grammar/parser level;
uniqueness enforcement (within or across enums) is left to the expander
(T-0225).

The `enum_ref` rule uses `${}` (compound-atomic) to prevent whitespace between
the enum name, dot, and member name.
