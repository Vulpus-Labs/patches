---
id: "0967"
title: VS Code — JS layout + render of patch-graph JSON
priority: high
created: 2026-05-28
---

## Summary

Make the VS Code patch-graph panel consume `patches/graphJson` (0966) and lay
out + render the graph with a JS engine into HTML with embedded SVG, replacing
the current static `innerHTML` SVG blob from `patches/renderSvg`. The diagram
becomes interactive: pan/zoom and click-to-source via provenance spans.

## Acceptance criteria

- [ ] Webview requests `patches/graphJson` (replacing `requestSvg`/`renderSvg`
      in `patches-vscode/src/extension.ts`).
- [ ] JS layered layout (`elkjs` or `dagre` — decide against the old SVG layout
      for parity) positions nodes; render to HTML + embedded SVG.
- [ ] Visual parity-or-better with the old SVG: nodes labelled (id : type),
      ports with cable-kind styling, cables routed; summing-junction glyph for
      autosum collapse preserved.
- [ ] Interactivity: pan/zoom; click a node/cable jumps to source using the
      provenance spans in the JSON.
- [ ] Debounced refresh on document edit (preserve current behaviour); empty /
      invalid patches render gracefully (placeholder, not crash).
- [ ] Extension builds; manual check in a VS Code dev host: open a `.patches`
      file, show the panel, edit, confirm live refresh + click-to-source.

## Notes

- ADR 0079 (Phase 2), Epic E158. Depends on 0966.
- This is the parity gate for 0968 — don't retire the Rust SVG pipeline until
  this renders at parity.
- Layout-engine choice is Open question 1 on E158; record the decision here.
- UI work: verify in the browser/dev-host, not just by building (per repo
  testing guidance for frontend changes).
