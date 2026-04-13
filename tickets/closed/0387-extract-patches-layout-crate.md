---
id: "0387"
title: Extract patches-layout crate from clap GUI
priority: high
created: 2026-04-13
---

## Summary

Factor the Sugiyama-based graph layout out of `patches-clap/src/gui_vizia.rs`
into a new `patches-layout` crate. Both `patches-clap` (vizia renderer) and
the new SVG renderer (ticket 0389) must share one layout implementation so
the CLAP GUI and the LSP/CLI outputs look identical.

## Scope

- New crate `patches-layout` in the workspace.
- Dependencies: `rust-sugiyama` only. No audio, no DSP, no serde unless needed.
- Public API (sketch):
  - `struct LayoutNode { id: NodeId, width: f32, height: f32, label: String, input_ports: Vec<PortLabel>, output_ports: Vec<PortLabel> }`
  - `struct LayoutEdge { from: (NodeId, PortIdx), to: (NodeId, PortIdx) }`
  - `struct LayoutConfig { node_width, port_row_height, header_height, padding, vertex_spacing, graph_margin }`
  - `struct GraphLayout { nodes: Vec<PositionedNode>, edges: Vec<RoutedEdge>, bounds: Rect }`
  - `fn layout_graph(nodes: &[LayoutNode], edges: &[LayoutEdge], config: &LayoutConfig) -> GraphLayout`
  - `fn node_height(port_count: usize, config: &LayoutConfig) -> f32`
- Transposition (top-to-bottom → left-to-right) and component stacking stay
  inside `layout_graph`, matching current behaviour in
  `patches-clap/src/gui_vizia.rs` lines 89-222.
- Cable routing returns cubic Bézier control points; the renderer draws them.

## Refactor patches-clap

- Remove the duplicated constants and `layout_graph` from `gui_vizia.rs`.
- Convert the snapshot into `LayoutNode`/`LayoutEdge` inputs.
- Render from the returned `GraphLayout` using vizia Canvas.
- Visual diff against current build should be zero.

## Acceptance criteria

- [ ] `patches-layout` crate compiles with only `rust-sugiyama` as a non-std dep.
- [ ] `patches-clap` consumes `patches-layout` and its GUI renders identically.
- [ ] Layout constants and `node_height` live in `patches-layout` only.
- [ ] `cargo test -p patches-layout` covers a small fixture graph (stable positions).
- [ ] `cargo clippy` clean across workspace.

## Notes

- Keep port identification abstract — caller supplies labels and port counts;
  the crate does not know about module descriptors.
- Avoid pulling `patches-core` in if possible; duplicate the small `NodeId`
  newtype here rather than leaking types across the boundary.
