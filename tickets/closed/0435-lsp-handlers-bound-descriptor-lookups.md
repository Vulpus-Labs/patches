---
id: "0435"
title: Migrate LSP feature handlers to BoundPatch descriptor lookups
priority: medium
created: 2026-04-15
---

## Summary

`StagedArtifact.bound` caches a `BoundPatch` on every pipeline run
(ticket 0432) but hover, completions, analysis, and expansion handlers
still resolve module descriptors by calling `Registry::describe` or by
walking raw `FlatModule` fields. Route descriptor and port-kind lookups
through the cached bound graph instead: handlers consult
`BoundModule::as_resolved()` for a pre-validated descriptor + param
map, and `BoundConnection`/`BoundPortRef` for pre-resolved cable kinds
and poly layouts.

Eliminates redundant descriptor work per feature call, keeps handler
behaviour consistent with what the pipeline already validated, and
sets up the bound graph as the single source of truth for any future
semantic feature (inlay hints, peek expansion, signal-graph queries).

## Acceptance criteria

- [ ] `hover::compute_expansion_hover` takes `&BoundPatch` (or a view
      over it) and reads descriptors from `ResolvedModule.descriptor`
      rather than re-describing via registry.
- [ ] Completions use `BoundModule` to list port names/kinds and
      parameter descriptors; no direct `Registry::describe` calls in
      completion code paths that already have a bound graph.
- [ ] `analysis` and `expansion` handlers either consume the bound
      graph or document why they must stay on raw FlatPatch
      (e.g. reporting on authoring-layer constructs that don't survive
      binding).
- [ ] Feature behaviour unchanged: existing LSP tests still pass;
      `Registry` stays a dependency only where the bound graph is
      unavailable (tree-sitter fallback path, ad-hoc lookups).
- [ ] `#[allow(dead_code)]` on `StagedArtifact.bound` removed.
- [ ] `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

Independent of ticket 0433 (tree-sitter gating). Can land before or
after. Handlers that currently run on the TS fallback stay on
`SemanticModel` + `Registry` — only the pest-clean path migrates here.

Descriptor lookups inside `BoundModule::Unresolved` are simply not
available; handlers should degrade gracefully (skip the feature or
fall back to name-level suggestions) rather than assume resolution.
