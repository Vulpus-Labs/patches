# E043 — ADR 0020: Named channel aliases and enum declarations

## Goal

Implement ADR 0020 in `patches-dsl`: add enum declarations and named channel
aliases to the DSL surface syntax. All new syntax is erased in Stage 2; the
flat IR, interpreter, and audio engine are unchanged.

After this epic:

- `enum <name> { ident, ... }` declarations are valid at file level.
- Enum members are referenceable as `<enum>.<member>` in any scalar position.
- A shape arg may accept an alias list `channels: [drums, bass, guitar]`,
  which resolves to the integer count and defines a per-instance alias map.
- Port and parameter indices accept bare identifiers resolved against the
  instance alias map: `mix.in[drums]`, `gain[bass]: 0.6`.
- `@<index>: { key: value, ... }` grouping blocks desugar to indexed param
  entries.
- The breaking change in bare-ident index semantics (template param → alias
  lookup) is applied and all existing patch sources migrated.

## Background

ADR 0020 is a pure Stage 2 / DSL concern. The key design points:

- **Enum values** are file-scoped integer constants (`drum.kick` → `0`).
- **Alias lists** are call-site features on module shape args; they determine
  arity and attach a named index map to the instance.
- **`@` blocks** are syntactic sugar; they desugar to indexed param entries
  before or during expansion.
- **Breaking change:** bare identifiers in `port[k]` and `param[k]` index
  position previously meant "template param ref" (`PortIndex::Param`). They
  now mean "alias lookup" (`PortIndex::Alias`). Template param refs in index
  position must use `<k>` syntax, consistent with everywhere else.

## Tickets

| #      | Title                                                                         | Priority |
|--------|-------------------------------------------------------------------------------|----------|
| T-0223 | Migrate bare-ident index refs to `<param>` syntax (breaking change prep)      | high     |
| T-0224 | Grammar, AST, and parser for `enum_decl`                                      | medium   |
| T-0225 | Expander: resolve `enum.member` references to integers                        | medium   |
| T-0226 | Grammar, AST, and parser for alias lists in shape args                        | medium   |
| T-0227 | Expander: build and resolve per-instance alias maps                           | medium   |
| T-0228 | Grammar, AST, parser, and expander for `@`-block desugaring                   | low      |
