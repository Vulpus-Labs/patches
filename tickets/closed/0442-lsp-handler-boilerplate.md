---
id: "0442"
title: LSP handler boilerplate extraction
priority: medium
created: 2026-04-15
---

## Summary

LSP handlers repeat the same plumbing:

- `source_id_for_uri` is copy-pasted in `hover.rs:109`, `peek.rs:115`,
  `inlay.rs:138`.
- `hover` (`workspace.rs:549-588`), `peek` (593-611), and `inlay_hints`
  (616-639) each lock state, fetch the document, call
  `run_pipeline_locked`, and destructure the same set of artifacts
  before delegating.
- Port-ref formatting (`PortIndex::Literal → "/{}"`, `Alias → "[{}]"`,
  `Arity → "[*{}]"`) is duplicated in `hover.rs:228-239` and
  `peek.rs:82-89`.

Extract:

1. `lsp_util::source_id_for_uri(uri) -> Option<SourceId>` — one copy.
2. `workspace::with_expansion_context(state, uri, f)` — locks, fetches,
   runs the pipeline, destructures the `StagedArtifact`, passes the
   ready-to-use bundle to `f`. Handlers become 1–3 lines each.
3. `shape_render::format_port_ref(&PortRef)` — one formatter, used by
   both hover and peek.

## Acceptance criteria

- [ ] Three copies of `source_id_for_uri` collapsed to one in
      `lsp_util.rs`.
- [ ] `with_expansion_context` exists; hover, peek, inlay handlers
      each reduce to locating the cursor span and calling the
      feature-specific renderer.
- [ ] Port-ref formatting lives in one place; no inline formatting
      blocks in `hover.rs` or `peek.rs`.
- [ ] Artifact destructuring is consistent across handlers (pick one
      style — tuple accessor or pattern match — and use it throughout).
- [ ] Existing LSP integration tests pass unchanged.
- [ ] `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

Part of E082. Pure refactor; no behaviour change expected.
