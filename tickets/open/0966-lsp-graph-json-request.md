---
id: "0966"
title: LSP patches/graphJson request
priority: high
created: 2026-05-28
---

## Summary

Add a `patches/graphJson` custom LSP request that returns the patch-graph JSON
(from `patches-graph-json`, ticket 0963) for the active document, wired through
the LSP's existing document/include source map — mirroring how
`patches/renderSvg` is wired today (`render_svg_pipeline` in
`patches-lsp/src/server.rs`).

## Acceptance criteria

- [ ] `patches-lsp` depends on `patches-graph-json`; custom method
      `patches/graphJson` registered alongside the existing request handlers.
- [ ] Pipeline: master path + in-memory sources → JSON, reusing the same
      `read_file`/source-snapshot machinery as `render_svg_pipeline`.
- [ ] Parse/expand errors return as diagnostics in the result (same shape as the
      SVG path: a result with diagnostics, not a hard error); partial/invalid
      patches still return a useful graph.
- [ ] Result type carries the JSON document + diagnostics; serialized over the
      LSP custom-method channel.
- [ ] Unit test mirroring `render_svg_pipeline_returns_*`: valid patch → JSON
      with expected modules; parse error → diagnostics.
- [ ] `patches/renderSvg` left in place for now (removed in 0968 once the JS
      renderer reaches parity).
- [ ] `just commit -p patches-lsp` green; `cargo clippy` clean.

## Notes

- ADR 0079 (Phase 2), Epic E158. Depends on 0963.
- Don't remove `patches-svg` from the LSP here — that's 0968, gated on JS-render
  parity (0967).
