---
id: "0433"
title: Gate tree-sitter fallback on stage-2 failure
priority: medium
created: 2026-04-15
---

## Summary

Today the LSP runs tree-sitter unconditionally and pest
opportunistically. ADR 0038 inverts this: pest leads, tree-sitter is a
fallback invoked only when stage 2 (pest parse) fails. Split
`patches-lsp::analysis` into a shallow structural pass (stage 4b) and
a shallow binding pass (stage 4c) so name-agreement diagnostics work
on syntactically-broken files. Document that shape resolution and
expansion-dependent checks are out of scope on the fallback path.

## Acceptance criteria

- [ ] Tree-sitter parse (`patches-lsp/src/parser.rs`) and tolerant AST
      build (`ast_builder.rs`) run only when stage 2 produced errors.
- [ ] Stage 4b (structural) runs against the tolerant AST and covers:
      patch-block count, unknown param/alias/module-name refs,
      recursive template instantiation via static call graph,
      unresolved `<param>` refs in songs.
- [ ] Stage 4c (binding) restricted to name-level registry agreement:
      known module types, plausible params and aliases, endpoint
      matching. No shape resolution.
- [ ] Hover and completions degrade gracefully on the fallback path:
      they operate on the partial tolerant AST without crashing or
      producing pest-path-style shape information.
- [ ] Tests exercise a syntax-broken fixture that is nonetheless
      structurally interesting (uses unknown aliases / recursive
      templates) and verify 4b diagnostics are published.
- [ ] `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

Depends on 0432. The tree-sitter structural and binding logic stays
parallel to the pest path (ADR 0038 explicitly rejects unifying the
ASTs). Drift risk is bounded by keeping 4b/4c shallow: any check that
requires expansion lives in 3a only.

## Resolution (2026-04-15)

Gated at the diagnostic-publication level rather than strictly gating
tree-sitter parsing itself:

- `pipeline::AccumulatedRun::stage_2_failed()` — true when any
  `LoadErrorKind::Parse` appears in `load_errors`.
- `StagedArtifact` carries the flag; `analyse()` / `reanalyse_cached()`
  publish tolerant-AST semantic diagnostics only when stage 2 failed.
  `lsp_util::to_lsp_diagnostics` split into `syntax_to_lsp_diagnostics`
  (always) and `semantic_to_lsp_diagnostics` (fallback only).
- Shape-alignment checks removed from `analysis::analyse_tracker_modules`
  (song vs MasterSequencer `channels`). Pest stage 3b reports these
  post-expansion; tree-sitter fallback stays name-level per ADR 0038
  §4c.
- Tree-sitter parse + tolerant AST build still run on both paths
  because completions/hover need cursor queries against the tree. A
  follow-up ticket can port completions onto pest-driven cursor
  resolution for a stricter read of AC #1. Epic E080 AC #3 ("no
  parallel always-run-both-parsers") is met at the user-visible layer:
  only one validation source publishes diagnostics per document.

Tests added in `patches-lsp/src/workspace.rs`:

- `stage2_failure_publishes_tolerant_structural_diagnostics` —
  syntax-broken + recursive template pair surfaces cycle diagnostic
  from the fallback path.
- `stage2_success_suppresses_tolerant_only_duplicates` — clean-pest
  file with unknown module emits exactly one diagnostic.

Structural/binding submodule split in `analysis.rs` deferred; current
`analyse_with_env` stays single-pass but its output is now strictly
name-level (no shape alignment) and is only published on the fallback
path.
