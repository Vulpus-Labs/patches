---
id: "0390"
title: LSP patches/renderSvg custom request
priority: high
created: 2026-04-13
---

## Summary

Add a custom JSON-RPC request `patches/renderSvg` to `patches-lsp`. Given a
document URI, the server loads the document (resolving includes), runs
`patches_dsl::expand` to obtain a `FlatPatch`, and returns the rendered SVG
from `patches_svg::render_svg`.

The render path deliberately does **not** go through `patches-interpreter`
or build a `ModuleGraph`. Rendering directly from `FlatPatch` means patches
with unknown module types, invalid parameter values, or type mismatches
still produce a useful graph visualisation.

## Protocol

Method: `patches/renderSvg`

Params:

```json
{ "textDocument": { "uri": "file://..." } }
```

Result:

```json
{
  "svg": "<svg ...>...</svg>",
  "diagnostics": [ /* optional: expand errors surfaced as structured diagnostics */ ]
}
```

On fatal error (document unknown, parse failure with no recoverable AST),
return a JSON-RPC error. On expand error where a partial `FlatPatch` can
be produced, return the partial SVG plus diagnostics. Future: include
LSP-level semantic diagnostics (unknown module types etc) in the response
so the VS Code panel can annotate.

## Scope

- Add deps to `patches-lsp`: `patches-dsl`, `patches-layout`, `patches-svg`.
  Do **not** add `patches-interpreter` or `patches-modules` for rendering.
- Register handler alongside existing LSP methods. Use `tower-lsp`'s
  custom-method mechanism
  (`#[tower_lsp::custom_method("patches/renderSvg")]` on an inherent
  impl, or register via `LspService::with_custom_method`).
- Pipeline inside handler:
  1. Look up `DocumentState.source` in `documents` map.
  2. `patches_dsl::parse(&source)` → AST file; on parse failure return JSON-RPC error.
  3. Resolve includes. Prefer reusing sources already in the LSP's
     `documents` map (include-loaded docs are tracked there); fall back
     to `patches_dsl::load_with` against disk for anything missing.
  4. `patches_dsl::expand(&file)` → `FlatPatch`. On expand error, surface
     diagnostics and still return an SVG if a partial patch is available.
  5. `patches_svg::render_svg(&flat, &SvgOptions::default())`.

## Acceptance criteria

- [ ] Server responds to `patches/renderSvg` with valid SVG for a well-formed
      document.
- [ ] Unknown module types / invalid params do not block rendering; the
      structural graph still renders.
- [ ] Expand/parse failures return structured diagnostics rather than panicking.
- [ ] Request handler runs on the tokio runtime like other handlers.
- [ ] Integration test drives the request end-to-end against a fixture.
- [ ] `cargo clippy` clean.

## Notes

- No caching in this ticket; recompute on each request.
- Leave room for future params (`theme`, `includePortLabels`) but do not
  wire them through VS Code yet.
