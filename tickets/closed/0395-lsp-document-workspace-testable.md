---
id: "0395"
title: Extract DocumentWorkspace from PatchesLanguageServer so non-protocol logic is testable without a Client
priority: medium
created: 2026-04-13
---

## Summary

`PatchesLanguageServer` owns a `tower_lsp::Client` alongside its analysis
state (documents map, parser, registry, nav index, include-loaded set).
Methods that don't touch the client — `resolve_includes`,
`analyse_and_publish`'s non-publishing half, include-graph traversal,
stale-GC — still require a constructed server, which means writing
tests for them currently requires a `tower_lsp::Client` and async
plumbing. Ticket 0394's LSP-level cycle test was deferred for this reason.

Separate the concerns: a `DocumentWorkspace` struct owns all analysis
state and exposes synchronous, `Client`-free methods returning data
(e.g. `Vec<Diagnostic>` instead of calling `publish_diagnostics`). The
LSP server becomes a thin adapter that holds `{ client, workspace }`
and translates protocol callbacks into workspace method calls, then
publishes results.

## Acceptance criteria

- [x] `patches-lsp/src/workspace.rs` added with `DocumentWorkspace`
      owning documents, parser, registry, nav_index, include_loaded.
- [x] `resolve_includes`, analysis/diagnostic construction, document
      insertion, and stale-GC moved off `PatchesLanguageServer`. None
      take `&Client`.
- [x] `PatchesLanguageServer` slimmed to `{ client, workspace }`; protocol
      methods delegate to workspace and publish returned diagnostics.
- [x] Workspace tests added (no `Client`): `cycle_two_file`,
      `self_include_is_cycle`, `missing_include_surfaces_diagnostic`,
      `diamond_load_loads_shared_once`, `grandchild_missing_surfaces_on_parent_directive`.
- [x] Existing LSP tests all pass (85 / 85).
- [x] `cargo clippy --workspace` clean; `cargo test --workspace` green.

## Bonus finding

While hoisting the stale-GC pass out of the recursive `resolve_includes`
into a top-level `purge_stale_includes`, uncovered a latent bug in the
pre-refactor code: stale-GC ran at every recursion level and seeded its
live set only from *that level's* direct includes. In a diamond `A→{B,C};
B→D; C→D`, C's recursion would prune B from `include_loaded` because
C's live set was only `{D}`. The reseeded `purge_stale_includes` now
walks from all editor-opened documents (docs minus include_loaded) and
runs once per `analyse` / `close` call. Covered by
`diamond_load_loads_shared_once`.

## Notes

Shape to aim for:

```rust
pub struct DocumentWorkspace {
    registry: Registry,
    documents: Mutex<HashMap<Url, DocumentState>>,
    parser: Mutex<Parser>,
    nav_index: Mutex<NavigationIndex>,
    include_loaded: Mutex<HashSet<Url>>,
}

impl DocumentWorkspace {
    pub fn new() -> Self { /* ... */ }
    pub fn analyse(&self, uri: &Url, source: String) -> Vec<Diagnostic>;
    pub fn close(&self, uri: &Url);
    pub fn completions(&self, uri: &Url, pos: Position) -> Vec<CompletionItem>;
    // etc.
}
```

Tests can then call `ws.analyse(&uri, source)` directly and assert on the
returned diagnostics without async, without a client, without tokio.

Unblocks cheap coverage for include-walk edge cases (ticket 0394's
deferred integration test) and any future LSP logic tests.
