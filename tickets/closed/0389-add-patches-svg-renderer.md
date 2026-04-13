---
id: "0389"
title: Add patches-svg renderer
priority: high
created: 2026-04-13
---

## Summary

New crate `patches-svg` that takes a `FlatPatch` (from `patches-dsl::expand`)
and produces an SVG `String`. Internally runs the shared `patches-layout`
(ticket 0387) and emits inline-styled SVG. Used by the LSP custom request
(0390), the CLI binary (0392), and the clap GUI (via the layout-adapter
helper, ticket 0388).

## Scope

- Dependencies: `patches-layout`, `patches-dsl`. No audio, no interpreter,
  no modules, no registry.
- Public API:
  - `fn render_svg(patch: &patches_dsl::FlatPatch, opts: &SvgOptions)
    -> String`
  - `fn flat_to_layout_input(patch: &patches_dsl::FlatPatch)
    -> (Vec<LayoutNode>, Vec<LayoutEdge>)` — re-used by clap GUI to feed
    `patches-layout` directly.
  - `struct SvgOptions { theme: Theme, include_port_labels: bool, embed_css: bool }`
    with sensible defaults.
  - `enum Theme { Light, Dark }` — palette mirrors `gui_vizia.rs` paints
    (lines 307-345) so outputs are visually consistent.
- Adapter rules (match current clap snapshot behaviour):
  - Node label = `"{id} : {type_name}"`.
  - Port labels = `port_label(name, index)` = `name` if `index == 0`
    else `"{name}/{index}"`.
  - Include only ports that participate in at least one connection.
  - Port row order = first appearance across `connections`.
- Node rendering: rounded rect + header with label + port rows with dots
  and labels.
- Cable rendering: cubic Bézier, `dx = (x1 - x0).abs() * 0.4`, matching
  `gui_vizia.rs:366-371`.
- `viewBox` sized to layout bounds plus a margin. Standalone, inline CSS
  when `embed_css`, per-element `style=` attributes otherwise.
- XML-escape `<`, `>`, `&`, `"`, `'` in all label/port text.

## Acceptance criteria

- [ ] `render_svg` produces valid XML (dev-dep `quick-xml` used in a test
      to parse and walk the output).
- [ ] Golden-style test: a small fixture `FlatPatch` renders to a string
      containing expected node labels, port labels, and a `<path d="M ... C ...">`
      per connection.
- [ ] Empty `FlatPatch` renders a minimal valid SVG with no nodes/edges.
- [ ] No panics / `unwrap` / `expect` in library code.
- [ ] `cargo clippy` clean.

## Notes

- Prefer inline `<style>` CSS over per-element attributes; gated by
  `SvgOptions::embed_css`.
- `font-family: sans-serif` for portability outside the editor.
- No FlatPatch mutation, no I/O.
