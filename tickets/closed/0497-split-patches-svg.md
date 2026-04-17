---
id: "0497"
title: Split patches-svg lib.rs
priority: medium
created: 2026-04-16
---

## Summary

`patches-svg/src/lib.rs` is 1068 lines covering SVG emission,
FlatPatch → layout-input lowering, and node/edge hint enrichment.
`layout` already lives in a submodule.

## Acceptance criteria

- [ ] Add submodules:
      `flat_to_layout.rs` (flat_to_layout_input, port_label,
      resolve_descriptor),
      `hints.rs` (enrich_node_hints, enrich_edge_hints,
      build_node_hint, build_edge_hint),
      `render.rs` (SVG string emission).
- [ ] `render_svg`, `SvgOptions`, `SvgTheme`, and other public
      items stay in `lib.rs` (or are re-exported from it).
- [ ] `lib.rs` under ~300 lines.
- [ ] `cargo build -p patches-svg`, `cargo test -p patches-svg`,
      `cargo clippy` clean.

## Notes

E086. Public `render_svg` signature unchanged.
