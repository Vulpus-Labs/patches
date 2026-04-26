---
id: "0695"
title: Tree-sitter grammar + highlights for tap targets
priority: high
created: 2026-04-26
---

## Summary

Mirror the pest tap-target grammar (ticket 0694) in the tree-sitter
grammar used by `patches-lsp` and the VS Code extension, with
highlight queries so `~`, tap component names, qualified keys, and
tap names colour correctly. Tree-sitter must parse partial / invalid
tap targets gracefully so editing mid-line doesn't break highlighting
across the rest of the file.

## Acceptance criteria

- [ ] `patches-lsp/tree-sitter-patches/grammar.js` adds productions
  for `tap_target`, `tap_components`, `tap_type`, `tap_name`,
  `tap_params`, `tap_param`, `tap_qualifier`, `tap_param_key`.
- [ ] Compound `~a+b+c(...)` parses as `tap_components` with multiple
  `tap_type` children.
- [ ] Qualified keys (`meter.window`) and unqualified keys (`window`)
  both parse as `tap_param`, with `tap_qualifier` present or absent.
- [ ] Permissive partial parse: an in-progress `~me` or `~meter(`
  produces an `ERROR` node localised to the tap target, not a cascade
  through the rest of the file.
- [ ] `highlights.scm` updated:
  - `~` → `@punctuation.special`
  - `tap_type` → `@function.special` (or chosen consistent class)
  - `tap_qualifier` → `@property`
  - `tap_param_key` → `@variable.parameter`
  - `tap_name` → `@variable`
- [ ] Corpus tests under `tree-sitter-patches/test/corpus/` for
  simple, compound, qualified, unqualified, with-gain, partial-parse
  recovery.
- [ ] `tree-sitter test` green; the regenerated parser builds
  cleanly.

## Notes

Tree-sitter's job here is *recognition*, not validation. Don't try to
enforce the closed component set or qualifier/component matching at
the grammar level — those produce LSP diagnostics in ticket 0698, run
against the pest parser per memory `project_lsp_expansion_hover`.

Lands in lockstep with 0694. Pick the same node names in both
grammars where possible to make the LSP wire-up in 0698 trivial.

## Cross-references

- ADR 0054 §1 — surface syntax.
- Memory: `project_lsp_expansion_hover` — pest runs alongside
  tree-sitter in the LSP for semantic features.
- E118 — parent epic.
