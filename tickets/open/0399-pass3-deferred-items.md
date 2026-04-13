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

- [ ] Refactor `patches-lsp/src/workspace.rs::resolve_includes` to call
  `patches_dsl::load_with` with an in-memory-aware source closure,
  instead of reimplementing cycle detection, canonicalization, and
  diamond dedup.
- [ ] Remove the `IncludeFrontier<Url>` outer walk in favour of the
  loader's `IncludeFrontier<PathBuf>`; translate paths ↔ URIs at
  the LSP boundary only.
- [ ] Preserve LSP-specific behaviour: keep analysed documents in the
  workspace map; respect `include_loaded` book-keeping so cleanup
  still works.
- [ ] Drop the `canonicalize()`-requires-disk quirk so unsaved editor
  buffers can satisfy includes without a first save.
- [ ] Verify: all existing workspace tests pass; add one for the
  "unsaved include target" case that currently fails.

**Depends on:** D1 (landed — `LoadErrorKind` is now structured, so the
LSP can translate loader errors to spans without string parsing).

### D5 — Resolved `FlatSong` in `FlatPatch`

- [ ] Replace `FlatPatch::songs: Vec<SongDef>` (unresolved AST form)
  with `FlatSong` carrying resolved pattern indices:
  `Vec<Vec<Option<PatternIdx>>>`, matching `FlatPatternDef`'s shape.
- [ ] Move the `SongCell::Pattern` / `SongCell::ParamRef` → index
  resolution out of `patches-interpreter` and into the expansion stage
  (`patches-dsl::expand`). After expansion, `ParamRef` cells cannot
  remain — flatness should enforce that.
- [ ] Update `patches-interpreter::build_tracker_data` to consume the
  resolved form; drop the re-walk of `SongCell` variants.
- [ ] Update downstream consumers (`patches-svg`, any tooling) to the
  new shape.

**Rationale:** the FlatPatch invariant is "fully resolved, ready for
the interpreter". A pre-expansion AST type leaking through is a bug-
magnet.

### D3 — Cross-check DSL AST and LSP AST drift

- [ ] Add a compile-time cross-reference (either a golden test under
  `patches-lsp/tests/` or a small assertion macro) that flags when a
  variant is added to `patches_dsl::ast` but missing from
  `patches_lsp::ast`, or vice-versa.
- [ ] The LSP AST is intentionally a tolerant mirror (tree-sitter
  input, Option-wrapped fields), so exact shape parity isn't the goal
  — the check should ensure the *set of kinds* stays aligned.
- [ ] Document the invariant in `patches-lsp/src/ast.rs` module
  docs: "any new DSL AST variant needs an LSP counterpart (possibly
  with Option fields)".

**Rationale:** the two ASTs will silently diverge if only one side is
updated. The review flagged `Scalar::Ident` (LSP-only) as an example;
that one was collapsed in batch C, but the structural risk remains.

## Notes

Original review at passes 1–3: see `tickets/open/0393-*.md`,
`tickets/closed/0394-*.md`, and the pass-3 review report in the
working session on 2026-04-13. D4 (`ExpandCtx` struct) was explicitly
excluded — it rides on the DSL type-check split work (see memory
`project_dsl_type_split`).
