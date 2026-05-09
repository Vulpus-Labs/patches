---
id: "0843"
title: Pest grammar — stereo module sugar
priority: medium
created: 2026-05-08
closed: 2026-05-09
epic: E140
adr: 0070
---

## Summary

Extend `patches-dsl/src/grammar.pest` to parse the `stereo` keyword
prefix on `module_decl`. After the redesign discussed on 2026-05-09 the
sugar's grammar surface is just that single token: side-specific params
reuse the existing `at_block` form (`@l: { ... }` / `@r: { ... }`)
inside the regular `param_block`, and channel selectors on ports reuse
the existing `port[l]` / `port[r]` named-index form. No new at-block
shape, no `.l` / `.r` accessor, no second param block.

The expander work (recognising `@l` / `@r` and `[l]` / `[r]` against a
stereo module's declared kind, and emitting the splitter / per-side mono
instances / joiner) is deferred to ticket 0844; until that lands the
validator rejects `is_stereo: true` with `StereoNotYetDesugared`
(`ST0038`) so the surface is reservable without a runtime path.

## Acceptance criteria

- [x] `module_decl` accepts an optional leading `stereo` token; AST node
      records `is_stereo: bool`.
- [x] Word-boundary lookahead on the `stereo` keyword identical to
      `bool_lit`, so identifiers like `stereo_in` are not consumed.
- [x] Pest unit tests cover: bare `stereo module x : Foo`, with shape,
      with `@l` / `@r` per-channel at_blocks inside the shared param
      block, `port[l]` / `port[r]` channel selectors, and the
      `stereo_in` word-boundary case.
- [x] Validator rejects `is_stereo: true` with `ST0038` until 0844.
- [x] Existing corpus and integration tests still pass.

## Notes

The constraint that **stereo modules must themselves be single-channel**
(no `Foo(N)` shape arg producing a multi-channel module) is enforced by
the binding stage against the module descriptor — that is 0844's
responsibility, not 0843's, since 0843 has no descriptor in scope.

Tree-sitter parity (ticket 0845) depends on this ticket landing first
(or in lock-step), since the corpus driver fails the build on any
divergence between pest and tree-sitter parses. With the redesign the
tree-sitter delta is a single keyword.

The first paragraph of `grammar.pest` says "Any change here MUST also
update patches-lsp/tree-sitter-patches/grammar.js AND add a corpus
entry"; that work is broken out into ticket 0845.
