---
id: "E072"
title: Patch SVG rendering (LSP + CLI + VSC panel)
created: 2026-04-13
tickets: ["0387", "0388", "0389", "0390", "0391", "0392"]
---

## Summary

Expose the expanded patch graph as an SVG document from three entry points:
a standalone CLI binary for scripted/doc usage, a custom LSP request
consumed by the VS Code extension (side-panel webview), and the existing
clap GUI (migrated to the shared layout). The layout algorithm currently
lives inside `patches-clap/src/gui_vizia.rs` (Sugiyama via `rust-sugiyama`,
transposed to left-to-right, cubic-Bézier cables) and is coupled to vizia's
Canvas API. Extract it into a backend-agnostic crate so every consumer
shares one layout.

Rendering is driven directly from `patches_dsl::FlatPatch`, not from a
`ModuleGraph`. This removes `patches-interpreter` and the module registry
from the render path entirely, simplifies the dep graph for `patches-svg`
and its consumers, and means partially-invalid patches (unknown modules,
type errors) still render a useful graph — valuable for the live VS Code
panel.

## Goals

- Single source of truth for patch graph layout (clap GUI, LSP, CLI).
- SVG renderer with no audio/DSP/interpreter dependencies.
- LSP custom request `patches/renderSvg` returning SVG for the active document.
- VS Code command + side-panel webview displaying the SVG, updating on edit.
- CLI binary for rendering a `.patches` file to an SVG on disk or stdout.

## Non-goals

- Interactive editing of the graph.
- Replacing the vizia GUI in `patches-clap`.
- Auto-routing cables around nodes (reuse existing Bézier routing).

## Tickets

| Ticket | Title                                                       | Priority |
| ------ | ----------------------------------------------------------- | -------- |
| 0387   | Extract `patches-layout` crate from clap GUI                | high     |
| 0388   | Switch clap GUI to FlatPatch-driven layout; delete snapshot | medium   |
| 0389   | Add `patches-svg` renderer                                  | high     |
| 0392   | `patches-svg-cli` binary                                    | medium   |
| 0390   | LSP `patches/renderSvg` custom request                      | high     |
| 0391   | VS Code: Show Patch Graph command + webview panel           | high     |

## Phased ordering

**Phase 1 — shared layout foundation (clap parity):**

- 0387 extracts `patches-layout`; 0388 migrates clap GUI to consume it via
  `FlatPatch` and deletes `PatchSnapshot`. Clap must render identically
  before anything downstream lands. 0388 depends on 0387; also depends on
  `flat_to_layout_input` from 0389 — so in practice 0389's adapter fn
  lands alongside 0388, and 0389's SVG renderer lands immediately after.

**Phase 2 — SVG renderer + CLI:**

- 0389 adds `patches-svg` with `render_svg(&FlatPatch, &SvgOptions)` and
  the shared `flat_to_layout_input` helper. 0392 adds the CLI binary that
  exercises the renderer end-to-end with a real `.patches` file and
  `--include-path` resolution. 0392 independent once 0389 lands.

**Phase 3 — LSP request:**

- 0390 adds `patches/renderSvg` to `patches-lsp`, wired through the
  existing document/include map. Depends on 0389 only.

**Phase 4 — VS Code panel:**

- 0391 adds the webview panel, command, and debounced refresh. Depends
  on 0390.

## Dependencies summary

- 0387 blocks 0388, 0389.
- 0389 blocks 0390, 0392 (and the adapter it exposes is needed by 0388).
- 0390 blocks 0391.
- 0392 independent of 0390/0391 once 0389 lands.
