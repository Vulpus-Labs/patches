---
id: "0399"
title: Pass-3 deferred items (loader unification, FlatSong, AST drift)
priority: medium
created: 2026-04-13
---

## Summary

Three findings from the pass-3 review (API shape / taste / layering) that
are too entangled with planned work or too architectural for the quick-win
batches already landed. Grouped here so they're tracked but not lost.

Quick wins and medium fixes were landed under commits [batch A–I] on
2026-04-13. This ticket covers L5, D5, D3 from that review.

## Acceptance criteria

### L5 — Fold `DocumentWorkspace::resolve_includes` into `load_with`

- [x] **Partial.** The `canonicalize()`-requires-disk quirk is fixed:
  include resolution in `workspace.rs` and its helpers
  (`direct_include_uris`, `purge_stale_includes`) now use
  `patches_dsl::include_frontier::normalize_path` for lexical
  normalisation, so unsaved editor buffers satisfy includes. New test
  `editor_buffer_satisfies_include_without_disk_save` covers this.
- [ ] **Not done — reassessed as not worth the churn.** See Notes for
  the architectural reason the full merger is deferred again.

### D5 — Resolved `FlatSong` in `FlatPatch`

- [x] `FlatPatch::songs` is now `Vec<FlatSongDef>` where `FlatSongDef.rows:
  Vec<FlatSongRow>` and `FlatSongRow.cells: Vec<Option<PatternIdx>>`.
- [x] `patches_dsl::expand::resolve_songs` performs the name→index
  resolution as a final pass after all patterns (top-level and
  template-local) are collected. `SongCell::ParamRef` cells surviving
  expansion are rejected with `ExpandError`. `FlatPatch.patterns` is
  now sorted alphabetically by qualified name for a stable index space.
- [x] `patches_interpreter::build_tracker_data` consumes the resolved
  form; the unknown-pattern re-walk is gone. Pattern existence is an
  expansion invariant, so the interpreter only validates per-column
  step/channel consistency.
- [x] Downstream: `patches-svg` does not touch `FlatPatch.songs`; no
  other consumers. Tests in `patches-dsl/tests/expand_tests.rs` and
  `patches-interpreter/src/lib.rs` updated to the new shape.

**Rationale:** the FlatPatch invariant is "fully resolved, ready for
the interpreter". A pre-expansion AST type leaking through is a bug-
magnet.

### D3 — Cross-check DSL AST and LSP AST drift

- [x] `patches-lsp/src/ast.rs` now contains a `drift` test module that
  exhaustively `match`es every DSL enum (`Scalar`, `Value`,
  `ShapeArgValue`, `ParamIndex`, `AtBlockIndex`, `ParamEntry`,
  `PortLabel`, `PortIndex`, `Direction`, `Statement`, `ParamType`,
  `StepOrGenerator`, `SongCell`, `RowGroup`, `PlayAtom`, `PlayBody`,
  `SongItem`). Adding a new DSL variant forces an explicit triage arm
  — either `LSP: <counterpart>` or `LSP: not mirrored — <reason>`.
  The test body is trivial; the compile-time exhaustiveness check is
  the real enforcement.
- [x] Intentional-divergence arms spell out *why* the LSP does not
  mirror a given variant (e.g. play/section composition, nested
  inline patterns) so future contributors understand the boundary.
- [x] The module docstring on `patches-lsp/src/ast.rs` documents the
  invariant: any new DSL AST variant needs an LSP counterpart or an
  explicit "not mirrored" entry.

**Rationale:** the two ASTs will silently diverge if only one side is
updated. The review flagged `Scalar::Ident` (LSP-only) as an example;
that one was collapsed in batch C, but the structural risk remains.

## Notes

Original review at passes 1–3: see `tickets/closed/0393-*.md`,
`tickets/closed/0394-*.md`, and the pass-3 review report in the
working session on 2026-04-13. D4 (`ExpandCtx` struct) was explicitly
excluded — it rides on the DSL type-check split work (see memory
`project_dsl_type_split`).

### L5 deferral (second pass)

Attempted the full refactor; the walking logic in `workspace.rs` and
`loader.rs` share an `IncludeFrontier` (already extracted to
`patches-dsl::include_frontier`), but everything else at each node
differs:

- `load_with` parses with **pest** and merges into a single
  `patches_dsl::ast::File`; it performs global template/pattern/song
  name-collision detection as it merges.
- `workspace.rs` parses with **tree-sitter** (for editor-tolerant
  partial parses), builds a `patches_lsp::ast::File`, caches the
  tree-sitter tree + analysis model per document, maintains
  forward/reverse include edges, and emits span-tied diagnostics; it
  does not merge files because each document is analysed in
  isolation.

Two different parsers, two different AST types, two different
per-node products. Expressing the LSP walk in terms of `load_with`
would require either double-parsing (pest + tree-sitter) or pushing a
callback trait through `load_with` that abstracts over the per-node
work — both are net worse than the current duplication. The
frontier/cycle/normalisation pieces that genuinely *are* shareable
already live in `patches-dsl::include_frontier`.

What was achievable and shipped: the `canonicalize()`-requires-disk
quirk was the concrete user-visible complaint in the original pass-3
report, and it is now fixed. Re-opening a broader unification should
be tied to a future change that unifies the two parsers or reshapes
the LSP's analysis pipeline, not done speculatively.
