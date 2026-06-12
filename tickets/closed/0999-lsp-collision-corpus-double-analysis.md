---
id: "0999"
title: "LSP: unscoped_index collisions, missing ts_error corpus quadrant, double analysis"
priority: medium
created: 2026-06-11
---

## Summary

1. **Wrong hover/completion on cross-template name collisions.**
   `patches-lsp/src/analysis/mod.rs:149-155` builds `unscoped_index`
   (leaf instance name → `ScopeKey`) with last-write-wins inserts.
   Two templates both defining `osc` → `get_descriptor()` fallback
   (`mod.rs:51-56`) serves the wrong module's descriptor to hover and
   completions. Common names (`osc`, `env`, `vca`) make collisions
   likely.
2. **Syntax corpus has zero `ts_error` entries.** Of 98 entries: 84 `ok`,
   12 `both_error`, 2 `pest_error`, 0 `ts_error`, 0 `expand_error`. The
   `ts_error` quadrant — tree-sitter accepts what pest rejects — is the
   drift scenario the corpus exists to catch; its detection surface is
   empty.
3. **Double analysis on watched-file change.**
   `workspace/lifecycle.rs:184-211` (`refresh_from_disk`) runs a full
   `analyse_with_env` with an empty template environment, writes the
   placeholder, then `reanalyse_cached` immediately re-analyses with the
   real environment and overwrites it. Pure CPU waste per
   `didChangeWatchedFiles`.

## Acceptance criteria

- [ ] `unscoped_index` either keys on (template, name), stores multiple
      candidates and disambiguates by cursor scope, or refuses the
      fallback on ambiguity (no wrong answer served). Test with two
      templates sharing a leaf module name.
- [ ] Corpus gains `ts_error` entries (start with known
      pest-rejects/tree-sitter-accepts constructs; minimum a handful) and
      at least one `expand_error` entry, per the syntax-corpus policy.
- [ ] `refresh_from_disk` inserts a parse-only placeholder and defers
      model construction to `reanalyse_cached` (single analysis per
      change).
- [ ] Stale docs from the same review fixed in passing: `expansion.rs:5-7`
      (describes a replaced lazy design), `analysis.rs:85` ("used by
      tests and feature handlers" — tests only), test fixtures using
      invalid `out.in_left` port names (ADR 0059: `AudioOut` has stereo
      `in`).

## Notes

Item 1 is the only user-visible correctness bug; 2 is the long-term
guard; 3 is perf only. Severity order in the summary is deliberate.

## Resolution (2026-06-11)

1. **unscoped_index collisions** → refuse-on-ambiguity (option (a)/(c)).
   When a leaf instance name appears in more than one template scope, it's
   dropped from `unscoped_index` entirely, so `get_descriptor`'s bare-name
   fallback misses rather than serving an arbitrary (possibly wrong)
   descriptor. Test `ambiguous_unscoped_name_not_served_by_fallback`.
2. **Corpus quadrants** — added 3 `ts_error` entries (unclosed tap paren,
   dangling arrow, `module name :` missing type) and 1 `expand_error`
   entry (`~meter(...)` as a cable source → ST0044). Fixed the
   `expand_error` extraction in `syntax_corpus.rs`: it scraped the Debug
   output for `ST####` but `ExpandError`'s Debug prints the variant name,
   so it never matched — now reads `err.code.as_str()`. (Also repaired a
   pre-existing `ok` entry in `tracker_tie_spread.corpus` that 0998's cv2-
   ramp rejection invalidated.)
3. **Double analysis** — `refresh_from_disk` now seeds the fresh
   `DocumentState` with `SemanticModel::empty()` (a cheap placeholder)
   instead of running a full `analyse_with_env`; `reanalyse_cached`
   rebuilds the real model moments later, so only one analysis runs per
   `didChangeWatchedFiles`. Added `DeclarationMap: Default`.
4. **Stale docs** — `expansion.rs` "lazy on feature-handler demand" → "runs
   in run_pipeline_locked, cached"; `workspace/analysis.rs` `ensure_flat`
   "used by tests and hover" → "test-only (allow(dead_code))"; fixed the
   invalid `out.in_left` → `out.in` fixtures in analysis tests
   (descriptors/scan/navigation). Workspace snapshot fixtures with
   `in_left` left untouched (would force snapshot regen) — noted for a
   later pass.

`cargo test -p patches-lsp` green (189 + corpus); clippy clean.
