---
id: "0968"
title: Retire Rust SVG pipeline (drop patches/renderSvg, patches-svg from LSP)
priority: medium
created: 2026-05-28
---

## Summary

Once the JS renderer (0967) is at parity, remove the in-process Rust SVG path:
delete the `patches/renderSvg` LSP request and drop `patches-svg` from the LSP
dependency tree, taking `rust-sugiyama`/`petgraph`/`log` with it (ADR 0079 §3).
Decide whether `patches-svg` is deleted outright or demoted to a standalone
JSON-consuming doc-render CLI.

## Acceptance criteria

- [ ] `patches/renderSvg` removed from `patches-lsp` (method registration,
      `render_svg_pipeline`, `RenderSvg*` types, `empty_svg`, related tests).
- [ ] `patches-svg` removed from `patches-lsp/Cargo.toml`.
- [ ] `cargo tree -p patches-lsp` shows **no** `rust-sugiyama` / `petgraph`
      pulled via SVG (verify, paste evidence in the PR).
- [ ] VS Code extension no longer references `renderSvg` / the SVG result type
      (cleanup of the old code path).
- [ ] `patches-svg` decision applied: either deleted from the workspace, or
      demoted to a standalone CLI consuming the patch-graph JSON for docs
      (server-side SVG/PNG). Record which and why.
- [ ] Docs build still works if it consumed `patches-svg` output (or is updated
      to the new path).
- [ ] `just push` green; `cargo clippy` clean.

## Notes

- ADR 0079 (Phase 3), Epic E158. Depends on 0967 (parity gate).
- E158 Open question 2: keep vs delete `patches-svg`. Driven by whether the
  manual build still wants a Rust-side renderer.
- This is the dep-cleanup payoff; it's modest (sugiyama/petgraph/log) — the
  bigger wins were the JSON contract + test surface in E157.
