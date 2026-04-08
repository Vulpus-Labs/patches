---
id: "0223"
title: Migrate bare-ident index refs to <param> syntax (breaking change prep)
priority: high
created: 2026-03-30
---

## Summary

ADR 0020 changes the meaning of a bare identifier in port-index and param-index
position. Currently `mix.in[k]` (bare ident in port index) is parsed as
`PortIndex::Param("k")` and resolved as a template parameter. After ADR 0020
it will be `PortIndex::Alias("k")` — an alias lookup against the target
module's alias map. Template parameter references in index position must use
`<k>` syntax, consistent with their usage everywhere else in the grammar.

This ticket performs the mechanical migration before any new alias machinery is
added, so the invariant is established at a clean baseline:

- Rename `PortIndex::Param` → `PortIndex::Alias` in the AST.
- Rename the corresponding `ParamIndex` bare-ident variant to `Alias` (currently
  `ParamIndex` only has `Literal(u32)` and `Arity(String)` — bare-ident param
  index is not yet parsed, so this may be a no-op until T-0226/T-0227).
- Search all test fixtures, inline DSL strings, and example `.patches` files
  for `port[k]` or `param[k]` patterns where `k` is a bare identifier acting
  as a template param ref, and rewrite them to `port[<k>]` / `param[<k>]`.
- Update the expander: any path that matched `PortIndex::Param` now matches
  `PortIndex::Alias` but behaviour is unchanged — for now the expander should
  treat `Alias` identically to the old `Param` lookup (template env only, no
  alias map yet). A TODO comment marks where alias-map resolution will be
  added in T-0227.

After this ticket `cargo test` and `cargo clippy` pass with zero warnings.

## Acceptance criteria

- [ ] `PortIndex::Param` renamed to `PortIndex::Alias` in `patches-dsl/src/ast.rs`.
- [ ] All parser match arms updated to produce `PortIndex::Alias`.
- [ ] All expander match arms updated; expansion behaviour unchanged (falls back
      to template env lookup as before; alias-map lookup added in T-0227).
- [ ] All existing test fixtures and example patches that used bare-ident port
      index as a template param ref updated to `<param>` syntax.
- [ ] `cargo test` passes, `cargo clippy -- -D warnings` clean.

## Notes

The change to `ParamIndex` (bare ident in param index position) is not yet
parsed — the current grammar only accepts `nat` or `*ident` in `param_index`.
No migration is needed there until T-0226 adds the alias list feature.

See ADR 0020 §"Breaking change to bare-ident port index semantics" for
rationale.
