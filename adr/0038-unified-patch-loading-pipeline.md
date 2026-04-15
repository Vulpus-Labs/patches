# ADR 0038 — Unified patch loading and validation pipeline

**Date:** 2026-04-15
**Status:** accepted

## Amendment — 2026-04-15 (post-implementation)

Two parts of the decision were collapsed during implementation. Recorded
here so the ADR matches what shipped.

- **Error types.** The ADR called for "New `ExpandError` vs
  `StructuralError` split." In practice every expansion error *is* a
  structural error (unknown alias, unknown param, recursive template,
  etc.), and the param/alias environment needed to classify them only
  exists during expansion itself. Implementation keeps a single error
  type (`patches_dsl::ExpandError`, re-exported as `StructuralError`)
  carrying a `StructuralCode` classifier. Diagnostics consumers
  dispatch on the code. No distinct post-hoc `StructuralError` type.
- **Post-expansion structural pass.** The ADR described stage 3a as
  expansion *followed by* a structural validation pass on the
  `FlatPatch`. No such check exists: all structural classification
  happens inline during expansion where the scoping envs are in scope,
  and the post-hoc seam (`structural::check`, `pipeline::structural`)
  was removed as dead code. If a future check genuinely only needs a
  `FlatPatch` (e.g. reachability analysis), re-introduce the seam
  then.

---

## Context

Patch loading happens in three consumers — `patches-player`, `patches-clap`,
and `patches-lsp` — and today each composes the stages differently:

- Player/CLAP run include resolution → pest parse → template expansion →
  interpreter binding against the module registry. Any failure aborts the
  load.
- LSP runs tree-sitter first for tolerance, then attempts pest + expansion
  opportunistically (ticket 0418, ADR 0037) when the tree is clean. Binding
  against the module registry is partial and scattered across feature
  handlers.

This ordering is inverted relative to where the authoritative information
lives. Pest is the runtime parser and drives expansion and binding; its
errors are the ones that actually describe what's wrong with the patch.
Tree-sitter exists only as a fallback for syntactically broken files. When
pest succeeds LSP has the same information the player has and should use
it; when pest fails LSP falls back.

The stages are also not cleanly separated. Expansion errors (circular
template instantiation, unknown param/alias names, missing `patch` block)
currently surface as part of binding, so the fail-fast consumers can't
discriminate "file is structurally broken" from "module registry rejected
a node" — both arrive as generic interpreter errors. And the LSP has no
single place to attach diagnostics: some come from tree-sitter, some from
pest, some from partial binding, each with its own span conventions.

## Decision

Define a single loading pipeline with named stages. Every consumer runs
the same stages in the same order; they differ only in **where they stop
on failure** and **what they do with accumulated diagnostics**.

### Stages

1. **Load and resolve includes.** Read the root `.patches` file, resolve
   `include` directives transitively, validate the include graph (no
   cycles, all referenced files exist and parse as UTF-8).
   - Player/CLAP: fail fast.
   - LSP: record errors, continue with whatever files loaded.

2. **Pest parse.** Build the pest AST for each loaded file.
   - Failure is a syntax error. Go to stage 4 (tree-sitter fallback) in
     LSP; fail fast in player/CLAP.
   - Success: go to stage 3a.

3. **a. Template expansion to FlatPatch + structural checks.**
   Expand templates, produce `FlatPatch`. Structural validation runs
   here, pre-registry: circular template instantiation, unknown
   param/module/alias names within template bodies, exactly one `patch`
   block.
   - Player/CLAP: fail fast on any structural error.
   - LSP: record errors, continue to 3b with the partial FlatPatch if
     one was produced.

   **b. Registry binding.** Resolve each `FlatModule` against the module
   registry: module type exists, required params present and typed,
   port references match the resolved descriptor (including shape-
   dependent ports), cable endpoints agree on kind.
   - Player/CLAP: fail fast; otherwise produce a bound graph and hand it
     to the planner / hot-reload path.
   - LSP: produce a partial bound graph with as much info as possible
     plus accumulated diagnostics; this is the artifact hover, inlay
     hints, peek expansion, and cross-cell diagnostics consume.

4. **a. Tree-sitter parse (LSP only).** Only reached when stage 2 failed.
   No template expansion.

   **b. Tree-sitter structural checks (LSP only).** Most structural
   checks from stage 3a are reachable without expansion: patch-block
   count, unknown param/alias/module-name refs, recursive template
   instantiation (via static call graph), unresolved `<param>` refs in
   songs. Run these against the tolerant AST so the user gets the
   same name-agreement diagnostics on structurally-but-not-syntax-
   broken files. Expansion-dependent cases (errors that surface only
   after a specific call-site's substitution) are out of reach here
   and accepted as a known gap of the fallback path.

   **c. Tree-sitter registry binding (LSP only).** Name-level agreement
   check against the registry: known module types, plausible params and
   aliases, matched cable endpoints. Produces a degraded partial graph
   so completions and hover keep working on broken files.

The tree-sitter fallback is **parallel**, not shared, with stages 3a/3b.
Their ASTs are too different to abstract over: pest drives a fully
expanded `FlatPatch` with provenance, tree-sitter gives a tolerant CST
with no expansion. An attempt to unify them would produce a lowest-
common-denominator interface that serves neither path well. Duplicating
the structural and binding logic is cheaper than carrying the
abstraction; the duplication is bounded by keeping stages 4b and 4c
shallow (no shape resolution, no expansion-dependent cases).

### Diagnostics

Each stage emits structured diagnostics with source spans. LSP aggregates
across stages 1–3 (or 1, 2, 4 on the fallback path) and publishes a single
diagnostics set per document. Player/CLAP print the first failing stage's
diagnostics and exit.

### Caching and LSP cost

Stage 3a is the expensive one — full expansion on every keystroke would
be wasteful. Mitigations, all already in flight or trivially extended
from ticket 0418:

- Pest file cache keyed on source hash; expansion cache keyed on the
  set of contributing file hashes. Already in `DocumentWorkspace`.
- Expansion runs lazily on first feature-handler call, not on every
  `did_change`.
- Debouncing at the handler layer.
- Stage 3b artifacts (the bound graph and `PatchReferences` from ADR
  0037) cache with the same lifetime as the FlatPatch.

Empirically for LSP-sized patches the full pipeline runs in low
milliseconds; we accept that cost in exchange for eliminating the
current two-parser divergence in feature behaviour.

## Consequences

**Positive**

- One pipeline, three consumers. Stage boundaries are named and testable
  in isolation; integration tests cover each fail-fast point.
- LSP features run against the same bound graph the player runs, so
  hover/completions/diagnostics agree with runtime behaviour on any file
  pest can parse.
- Structural errors (circular templates, unknown aliases) become a
  distinct stage rather than being tangled with registry binding —
  clearer messages and cleaner error types.
- The tree-sitter path is explicitly a degraded fallback, not a parallel
  truth. Reduces the surface area of "which parser said what" bugs.

**Negative**

- Stage 4b duplicates a subset of stage 3b's binding logic against a
  different AST. Drift risk between the two: a new required param added
  to a module descriptor needs both binders updated. Mitigated by keeping
  stage 4b intentionally shallow — name-level agreement only, not full
  shape resolution.
- LSP pays full expansion cost on clean files. Mitigated by caching, but
  large patches with deep template nesting are the thing to watch.

**Neutral**

- Public APIs of `patches-dsl` and `patches-interpreter` shift only at
  the seams — this is mostly a reorganisation of how existing stages
  are composed and where errors are captured.
- Stage 3a's structural checks partly live in `patches-dsl::expand`
  today (unknown param/alias/module names inline); stage 3b's checks
  live in `patches-interpreter::build`. Lifting 3a out as a distinct
  pre-binding pass is a refactor, not a redesign.

## Blast radius

Current state was surveyed across the workspace. Changes group by
stage; "read" means the file informs the design but may not need
edits.

### Stage 1 — load and include resolution

Already cleanly separated; stays as-is, gains a diagnostic converter.

- `patches-dsl/src/loader.rs` — `load_with()`, `LoadResult`, `LoadError`
  (read; no changes expected).
- `patches-dsl/src/include_frontier.rs` — cycle detection (read).
- `patches-diagnostics/src/lib.rs` — add `LoadError` →
  `RenderedDiagnostic` converter.

### Stage 2 — pest parse

- `patches-dsl/src/parser.rs` — `parse()`, `parse_with_source()`,
  `ParseError` (read; spans already byte-offset).
- Call sites move behind the new pipeline entry point.

### Stage 3a — expansion + structural checks

The main refactor site. Pull structural checks out of `expand.rs` into a
distinct post-expansion pass so error types separate from binding.

- `patches-dsl/src/expand.rs` — split: keep mechanical expansion, move
  unknown param/alias checks (currently lines ~741–1240) and unknown
  module-name checks (~1181–1187) into a new `structural.rs` pass.
  Add explicit "exactly one patch block" check.
- `patches-dsl/src/ast.rs`, `flat.rs` — read (FlatPatch shape unchanged).
- New `ExpandError` vs `StructuralError` split, both provenance-carrying.

### Stage 3b — registry binding (pest path)

- `patches-interpreter/src/lib.rs` — `build()` / `build_with_base_dir()`.
  Rename errors so `InterpretError` only covers binding (unknown module,
  bad shape, missing/mistyped param, cable kind mismatch). Structural
  cases move to stage 3a's error type.
- `patches-interpreter/src/*` — any submodules currently producing
  structural errors migrate those to 3a.
- `ModuleGraph` output unchanged; this is the bound graph the ADR names.

### Stage 4 — tree-sitter fallback (LSP only, parallel)

Stays parallel; tightened so it only runs when stage 2 fails. Gains a
distinct structural sub-stage (4b) so name-agreement diagnostics work
on syntax-broken files, mirroring stage 3a where reachable.

- `patches-lsp/src/parser.rs` — tree-sitter entry (read).
- `patches-lsp/src/ast_builder.rs` — tolerant AST build (read).
- `patches-lsp/src/analysis.rs` — split into a shallow structural pass
  (patch-block count, unknown param/alias/module-name refs, recursive
  template instantiation, unresolved `<param>` in songs) and a shallow
  binding pass (name-level registry agreement). No shape resolution.

### Consumer wiring

A new pipeline orchestrator (likely `patches-dsl::pipeline` or a thin
crate) exposes the staged entry points. Consumers call it:

- `patches-player/src/main.rs` — `load_patch()` and hot-reload loop
  (`run()` around lines 232–270) switch to the staged API; policy is
  fail-fast on any stage.
- `patches-clap/src/plugin.rs` — `load_or_parse()` and
  `compile_and_push_plan()`; `patches-clap/src/error.rs::CompileError`
  already discriminates Load|Parse|Expand|Interpret|Plan — map new
  stage boundaries onto it (structural becomes its own variant).
- `patches-lsp/src/workspace.rs` — `DocumentWorkspace`,
  `ensure_flat_locked`, `flat_cache`, `invalidate_flat_closure`.
  Replace `maybe_parse_pest` with the staged pipeline call; stages 1–3
  feed the primary path, stage 4 is invoked only on stage-2 failure.
- `patches-lsp/src/expansion.rs`, `hover.rs`, `completions.rs`,
  `analysis.rs`, `main.rs` — diagnostics publication aggregates across
  stages; feature handlers consume the bound graph (or partial graph on
  the fallback path).

### Diagnostics rendering

- `patches-diagnostics/src/lib.rs` — add converters for `LoadError`,
  `StructuralError`, `InterpretError`. Unify severity/code scheme across
  stages so LSP can publish one aggregated set.

### Tests

- `patches-dsl/tests/expand_tests.rs`, `parser_tests.rs` — split so
  expansion and structural checks are exercised independently.
- New `patches-dsl/tests/structural_tests.rs` for stage 3a.
- `patches-integration-tests/tests/dsl_pipeline.rs` — extend beyond the
  success path; add fail-fast cases per stage.
- `patches-lsp` — add an integration harness that drives the staged
  pipeline end-to-end against sample files (clean, syntax-broken,
  structurally-broken, binding-broken).
- `patches-clap`, `patches-player` — error-path tests per stage.
