---
id: "0855"
title: Retire Sum / PolySum / StereoSum modules
priority: low
created: 2026-05-09
epic: E142
---

## Summary

Final step of ADR 0071. With multi-source input ports doing the
summation natively (tickets 0853 + 0854), the `Sum`, `PolySum`, and
`StereoSum` modules earn no keep — their behaviour is the input port's
behaviour, with one extra `process()` frame and one extra cable hop on
top. Delete the three modules, remove them from the default registry,
and migrate every fixture / corpus / doc / SVG that named them to
direct fan-in.

## Acceptance criteria

- [ ] `patches-modules/src/sum.rs`, `poly_sum.rs`, and `stereo_sum.rs`
      deleted along with their unit tests.
- [ ] `patches-modules/src/lib.rs` no longer declares the three modules
      and no longer registers them in `default_registry`. The
      `default_registry_contains_all_modules` test in the same file
      drops the three names from its expected list.
- [ ] mdBook module reference (`docs/src/modules/`) drops the three
      pages; any cross-references in other module docs ("see also Sum")
      are deleted or rewritten.
- [ ] LSP syntax corpus
      (`patches-lsp/tests/syntax_corpus/`, per memory note
      "syntax-corpus-policy") loses any entry that exercises `Sum`-family
      shape semantics. Per-feature highlight / hover / parse fixtures
      that simply *used* a `Sum` for plumbing are rewritten to fan
      directly into the consumer.
- [ ] Any in-tree `.patches` example (under `patches-player/examples`,
      integration-test fixtures, drum-kit tutorials, etc.) that
      references `Sum(N)` / `PolySum(N)` / `StereoSum(N)` is rewritten.
      `git grep -nE 'module [a-zA-Z0-9_]+ : (Stereo|Poly)?Sum'` returns
      zero matches.
- [ ] `tools/align-tables.py` is run on any docs touched (per memory
      note `feedback_table_alignment.md`).
- [ ] `just push` green (this is the workspace-wide gate; the deletion
      ripples into more crates than the inner-loop subset covers).

## Notes

- This is intentionally the last step of E142. Keeping the modules
  alive through ticket 0854 means the multi-edge builder can be
  validated against the same fixtures the synthesised-Sum path used,
  with both code paths producing identical graphs. Deleting them now
  is a pure cleanup — the behaviour they encoded already lives in the
  input ports.
- If a user reaches for a *named* summing node for clarity, the
  recommended path is a one-line template:
  `template sum2(a: audio = 0, b: audio = 0) { in: a, b out: a + b }`.
  Documenting that pattern in the mdBook `templates` chapter is a
  nice-to-have but not in scope here.
- No deprecation window: pre-1.0, in-tree fixtures are the only known
  callers. External users can copy a `Sum` definition into their own
  patch repo if they need the exact module shape; the source is
  three small files (or was, until this ticket lands).
- Plugin-scanner / FFI surface unaffected — none of the three modules
  is exposed via FFI today.
