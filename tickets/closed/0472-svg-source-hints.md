---
id: "0472"
title: Source hints in rendered SVG
priority: medium
created: 2026-04-16
---

## Summary

`FlatPatch` carries `Provenance { site, expansion }` on every
`FlatModule`, `FlatConnection`, and `FlatPortRef`
(`patches-dsl/src/flat.rs:15-71`), but `render_svg`
(`patches-svg/src/lib.rs:63`) discards it. Wire provenance into
the SVG so hovering a node or cable in a browser/webview reveals
the source snippet and call-site chain.

## Acceptance criteria

- [ ] `render_svg` takes `&SourceMap` (new required param;
      both callers — `patches-svg/src/bin/patches-svg.rs:136` and
      `patches-lsp/src/server.rs:333` — already have one in scope).
- [ ] Each node `<g>` and each cable `<path>` carries:
      - a `<title>` child with the site snippet plus expansion
        trail (one line per `expansion` frame: `from <snippet>`).
      - `data-source-id`, `data-span-start`, `data-span-end`
        attributes for downstream tooling.
- [ ] Synthetic spans (`SourceId(0)`) emit no tooltip and no
      data-* attrs.
- [ ] Existing `patches-svg` tests pass; add coverage that the
      emitted SVG contains `<title>` and `data-span-start` for a
      simple patch, and that a synthetic-provenance fixture
      omits both.
- [ ] `cargo build`, `cargo test -p patches-svg`,
      `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

Browser-native `<title>` is zero-JS. Multi-line behaviour varies
by browser (Chrome collapses whitespace, Firefox preserves) —
good enough baseline; VSCode webview or docs site can layer a
styled overlay later using the `data-*` attributes.

Pairs with 0473 (cable styling by kind). Independent tickets —
either can land first.
