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

## Resolution (2026-06-11)

- `patches-lsp` now depends on `patches-graph-json`; the custom method
  `patches/graphJson` is registered in `main.rs` alongside
  `patches/renderSvg` and `patches/rescanModules`.
- `graph_json` handler + `graph_json_pipeline` mirror `render_svg` /
  `render_svg_pipeline`: same `sources_snapshot` + `read_file` machinery
  (in-memory docs first, disk fallback), `load_with` → `expand` →
  `graph_doc` → `to_json_pretty`.
- Parse/expand (and serialise) errors return as an error diagnostic
  alongside an empty-but-valid `GraphDoc` JSON (built from a real
  `GraphDoc`, so the shape can't skew), not a hard LSP error — partial /
  invalid patches still yield a usable result.
- `GraphJsonResult { json, diagnostics }` reuses the SVG path's diagnostic
  shape; serialised over the custom-method channel.
- Tests `graph_json_pipeline_returns_json_for_valid_patch` (asserts osc +
  vca modules present) and `graph_json_pipeline_returns_diagnostic_on_parse_error`
  (diagnostic + empty graph), mirroring the SVG pipeline tests.
- `patches/renderSvg` and `patches-svg` left in place (removal is 0968,
  gated on 0967 parity).

`just commit -p patches-lsp` green; clippy clean.
