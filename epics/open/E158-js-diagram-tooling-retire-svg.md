---
id: E158
title: JS diagram tooling over patch-graph JSON; retire Rust SVG pipeline
status: open
created: 2026-05-28
---

## Goal

Move patch-graph layout and rendering out of Rust to a JS consumer reading the
patch-graph JSON (from **E157**), then retire the in-process Rust SVG pipeline.
Design and rationale in **ADR 0079**. This epic is Phases 2 and 3; it depends
on E157 delivering the JSON emitter (`patches-graph-json`, ticket 0963).

Key decisions (ADR 0079):

- The LSP exposes `patches/graphJson` (the JSON contract). The VS Code webview
  lays out and renders with a JS engine (`elkjs`/`dagre` for layered layout)
  into HTML with embedded SVG — interactive (pan/zoom/click-to-source via
  provenance spans), replacing the static `innerHTML` SVG blob.
- `patches/renderSvg` and the `patches-svg` LSP dependency are removed only
  once the JS renderer reaches parity; `rust-sugiyama`/`petgraph`/`log` leave
  the LSP dep tree with it.

## Scope

**In:**

- LSP `patches/graphJson` custom request returning the patch-graph JSON for the
  active document, wired through the existing document/include map.
- VS Code: webview consuming the JSON, JS layered layout + render to HTML +
  embedded SVG; interactivity (pan/zoom, click-to-source from provenance);
  debounced refresh on edit.
- Retire the Rust SVG pipeline: remove `patches/renderSvg` from the LSP, drop
  `patches-svg` from the LSP dep tree (verify `rust-sugiyama`/`petgraph`/`log`
  gone via `cargo tree`). Either delete `patches-svg` or demote it to a
  standalone JSON-consuming doc-render CLI.

**Out (deferred / other epics):**

- The JSON emitter + golden harness — **E157** (must land first).
- Fully-validated `ModuleGraph` JSON tier — ADR 0079 Open question 2.
- Compiled patch artifact for player/CLAP — spike ticket 0969.

## Tickets

- [ ] [0966 — LSP `patches/graphJson` request](../../tickets/open/0966-lsp-graph-json-request.md)
- [ ] [0967 — VS Code: JS layout + render of patch-graph JSON](../../tickets/open/0967-vscode-js-diagram-render.md)
- [ ] [0968 — Retire Rust SVG pipeline (drop `patches/renderSvg`, `patches-svg` from LSP)](../../tickets/open/0968-retire-rust-svg-pipeline.md)

## Dependency order

```text
E157/0963 (JSON emitter) ──> 0966 (LSP request) ──> 0967 (JS render) ──> 0968 (retire SVG)
```

## Acceptance

- `patches/graphJson` returns the patch-graph JSON for the active document,
  updating on edit; partial/invalid patches still return a useful graph.
- The VS Code panel renders the graph from JSON with JS layout, at visual
  parity-or-better with the old SVG, and supports click-to-source via
  provenance spans.
- After 0968: `cargo tree -p patches-lsp` shows no `rust-sugiyama`/`petgraph`
  pulled via SVG; `patches/renderSvg` is gone; the LSP and VS Code build and the
  graph panel works end-to-end.
- `just push` green; `cargo clippy` clean.

## Open questions

1. **JS layout engine.** `elkjs` (layered, richer routing) vs `dagre`
   (lighter). Decide in 0967 against the existing SVG's layout for parity.
2. **Keep or delete `patches-svg`.** Delete vs demote to a JSON-consuming
   doc-render CLI (server-side SVG/PNG for the manual). Decide in 0968 based on
   whether the docs build still wants a Rust-side renderer.
