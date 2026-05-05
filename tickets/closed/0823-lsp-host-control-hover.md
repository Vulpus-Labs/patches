---
id: "0823"
title: LSP hover for host-control declarations and references
priority: low
created: 2026-05-05
epic: E135
depends_on: "0810"
---

## Summary

Add hover surfaces in `patches-lsp` for host-control declarations
and bare-name references, so editor users can inspect kind / fields
without leaving the call site.

## Acceptance criteria

- [x] Hover on a host-control declaration (`knob` / `slider` /
      `toggle` / `trigger` block) renders the kind plus the block's
      fields as a markdown table.
- [x] Hover on a bare-name reference resolves to the linked
      declaration and renders the same content (prefixed with the
      reference name as a heading link).
- [x] Unresolved bare-name reference returns an explanatory hover
      rather than `None`, so the editor can guide the user.
- [x] Tests at
      [patches-lsp/src/workspace/tests/host_control.rs](../../patches-lsp/src/workspace/tests/host_control.rs)
      cover all four hover surfaces.
- [x] `just inner -p patches-lsp` passes (161 tests).

## Resolution

- Tree-sitter grammar gained `host_control_block` (kind keyword,
  name ident, comma-separated `name: value` fields) and
  `host_control_ref` (bare-ident endpoint, low-precedence so
  `module.port` still wins). `_cable_endpoint` and `statement` add
  the new alternatives.
- Cursor classifier (`tree_nav.rs`) recognises `host_control_block`
  ancestors and `host_control_ref` nodes, returning two new
  `CursorContext` variants. `compute_hover` dispatches them to a
  new `hover::host_control` module that renders the kind/name
  header and a `field | value` markdown table.
- Reference hover walks the tree-sitter root for a sibling
  `host_control_block` whose `name` field matches the ident under
  the cursor; unresolved refs fall back to a brief explanatory
  message so the editor still surfaces a tooltip.
- `workspace::features::hover` short-circuits on host-control
  cursor variants before the expansion-aware path runs — without
  this, a hover on a bare-name reference would render the
  synthesised `~host_control.audio_out → flt.cv` cable rather than
  the user-facing declaration.
- `build.rs` now declares `cargo:rerun-if-changed` for the
  generated parser artefacts so future grammar regenerations
  actually rebuild the linked C parser without a manual `touch`.

## Notes

- Sits in [patches-lsp/src/hover/](../../patches-lsp/src/hover/);
  follow the existing per-construct hover module pattern.
- Split out from ticket 0813. See ticket 0822 for diagnostics.
