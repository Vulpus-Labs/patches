---
id: "0845"
title: Tree-sitter grammar parity for stereo module sugar
priority: medium
created: 2026-05-08
epic: E140
adr: 0070
depends-on: "0843"
---

## Summary

Mirror the pest grammar addition from ticket 0843 in the tree-sitter
grammar at `patches-lsp/tree-sitter-patches/grammar.js`, add a corpus
entry to enforce parity, and extend the highlight queries to colour
the new keyword.

After the 2026-05-09 redesign the new surface is a single token: an
optional `stereo` prefix on `module_decl`. Per-channel overrides
(`@l: { ... }`) and channel selectors on ports (`port[l]`) reuse rules
that already exist in both grammars; nothing to add for those.

The corpus driver in `patches-lsp/src/syntax_corpus.rs` runs both
parsers over every `*.corpus` file under `patches-lsp/tests/syntax_corpus/`
and fails on divergence. This ticket is the reason that file lives — it
must pass with the new syntax in both grammars.

## Acceptance criteria

- [ ] `module_decl` in `grammar.js` accepts an optional `stereo`
      keyword prefix matching the pest rule.
- [ ] Word-boundary handling matches pest: `stereo_in` does not
      tokenise as `stereo` + `_in`.
- [ ] New corpus file `patches-lsp/tests/syntax_corpus/stereo_module.corpus`
      with cases:
  - bare `stereo module x : Foo`
  - with shape and params
  - with `@l` / `@r` per-channel at_blocks inside the param block
  - with `port[l]` / `port[r]` channel selectors in cables
  - mixed bus + selector references in the same patch
  - corpus driver passes (pest and tree-sitter agree on parse trees).
- [ ] `patches-lsp/tree-sitter-patches/queries/highlights.scm` highlights
      the `stereo` keyword as a keyword.
- [ ] `tree-sitter test` in `patches-lsp/tree-sitter-patches/` passes
      for any inline test files that exercise the new rule.
- [ ] Tree-sitter generated artefacts (`src/parser.c`, etc.) are
      regenerated and committed if the project's convention is to
      check them in (verify against current state of
      `patches-lsp/tree-sitter-patches/src/`).

## Notes

If 0843 lands first, the corpus driver will fail until this ticket
lands — that's the intended forcing function for parity. Land the two
in close succession; a draft PR for 0845 alongside 0843 keeps reviewers
synchronised.

The tree-sitter grammar uses field names (`field("name", ...)`) that
the LSP's `tree_nav.rs` reads to navigate the syntax tree. Add a
`stereo` field on the keyword token of `module_decl` so `tree_nav`
can detect stereo decls without re-walking children. No port_ref
field is needed — selectors live on the existing `port_index` rule.
