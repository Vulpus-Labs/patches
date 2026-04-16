---
id: "E084"
title: DSL/LSP deep cleanup — completions, helpers, include graph
created: 2026-04-15
status: closed
closed: 2026-04-16
depends_on: ["E083"]
tickets: ["0463", "0465", "0467", "0468", "0469"]
closed_wontfix: ["0462", "0464", "0466", "0470", "0471"]
---

## Summary

Follow-on to E083. A deeper architectural review of `patches-dsl`
and `patches-lsp` initially surfaced ten issues; reality-checking
against the actual code closed five as won't-fix (work already
done or scope not paying for itself) and narrowed one. What
remains:

1. **Completions has two parallel cursor-classification paths.**
   `tree_nav::classify_cursor` (0457) is used for one context in
   completions; the rest fall through to an older walker. Hover
   and navigation dispatch uniformly on `CursorContext` — 0463
   finishes the parsed-input migration. Incomplete-input recovery
   via `scan_backward_for_context` stays as a documented
   fallback.

2. **Tree-sitter field extraction is ad hoc.** Magic strings
   (`"MasterSequencer"`) and `child_by_field_name("type")` +
   `node_text` patterns recur inline across handlers — a
   rename scatters edits. 0465 extracts `tree_util` helpers.

3. **StagedArtifact bundles diagnostics with artifacts.** Not a
   correctness issue; the pre-bucketing is the natural shape for
   one-shot publish. 0467 cleans up the separation of concerns at
   low priority.

4. **Three sibling HashMaps implicitly coordinate the include
   graph.** `included_by`, `includes_of`, `artifacts` with
   invariants in comments. 0468 introduces an `IncludeGraph`
   struct with documented invariants.

5. **Feature handlers swallow failures silently.** Debugging
   aids, not type-system work: 0469 adds `tracing::debug!` at
   early-return points.

## Closed-wontfix items (from this review)

- **0462 Collapse tolerant AST mirror** — LSP AST is a
  deliberately narrower subset, not a true mirror; drift test
  already eliminates silent-drift risk.
- **0464 Port introspection onto ResolvedDescriptor** — 0461
  already did this; remaining inline access is rendering
  iteration, not lookup.
- **0466 FlatPatchView facade** — `FlatPatch` is the published
  contract of `patches-dsl`; a trait wrapper adds ceremony
  without unlocking polymorphism anyone needs.
- **0470 Explicit phase DAG** — real DAG is nearly linear
  already; docstrings from 0456 are honest enough.
- **0471 Hover fallback lock** — misread; there is one lock
  cycle, not two.

No behavioural change. Pure structural cleanup where the work
pays for itself.

## Acceptance criteria

- [ ] Completions dispatches fully on `CursorContext` for parsed
      input; `try_completion_from_node` deleted;
      `scan_backward_for_context` retained with docstring
      explaining incomplete-input fallback.
- [ ] Tree-sitter field extraction helpers (`module_type_name`,
      `param_value_for_name`, etc.) extracted; `"MasterSequencer"`
      literal localised to one site.
- [ ] `StagedArtifact` holds artifacts only; diagnostics rendered
      at publish-time by `analyse` / `run_pipeline_locked`.
- [ ] `IncludeGraph` struct wraps `included_by` + `includes_of`
      with documented invariants and `add_edge`, `ancestors_of`,
      `rewrite_edges` methods; workspace coordinates one graph +
      one artifacts map.
- [ ] Every early-return `None`/`Vec::new()` in LSP feature
      handlers (completions, hover, peek, inlay, goto) carries a
      `tracing::debug!` naming the failure mode.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Tickets

| ID   | Title                                                          |
|------|----------------------------------------------------------------|
| 0463 | Unify parsed-input completions on CursorContext                |
| 0465 | Tree-sitter field extraction helpers                           |
| 0467 | Separate StagedArtifact caching from diagnostic rendering      |
| 0468 | IncludeGraph struct for workspace include coordination         |
| 0469 | Trace silent failure points in LSP feature handlers            |

## Out of scope

- New DSL or LSP features.
- Performance work (parallel analysis, incremental rebuild).
- Diagnostic rendering format changes (E082/0439).
- Folding `scan_backward_for_context` into `classify_cursor`
  (would require richer tree-sitter error recovery or
  text-inspection inside the classifier — separate concern).

## Priority guide

- High: 0463 (completion path) — biggest ongoing maintenance tax.
- Medium: 0465 (tree-sitter helpers), 0468 (include graph).
- Low: 0467 (StagedArtifact cleanup), 0469 (tracing).
