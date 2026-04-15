---
id: "0422"
title: LSP inlay hints for poly widths and indexed ports
priority: medium
created: 2026-04-15
---

## Summary

Render inlay hints beside template call sites in `.patches` files
showing the concrete shape (e.g. `channels=4`, `length=2048`) and
indexed-port ranges (e.g. `out[0..3]`) that the call expanded into.
The information is already computed during expansion and stored on
`FlatModule.shape` but is currently discarded after hover renders it.

See ADR 0037 and epic E078.

## Acceptance criteria

- [ ] LSP server registers `inlay_hint_provider` capability in
      `initialize` (`patches-lsp/src/main.rs`).
- [ ] `Workspace::inlay_hints_for_range(uri, range)` returns hints for
      template call sites whose authored span overlaps the requested
      range, by iterating `PatchReferences::call_sites`.
- [ ] Each hint aggregates the shapes of the modules emitted under the
      call site. When all emitted modules share a shape it is rendered
      as a single hint; when shapes differ the hint shows the range
      (e.g. `channels=2..4`) — implementation may start with the
      single-shape case and emit no hint when shapes diverge.
- [ ] Indexed-port ranges are rendered for descriptors with indexed
      ports, using the concrete shape rather than `ModuleShape::default()`.
- [ ] Shape-evaluation helpers used by hover today
      (`patches-lsp/src/analysis.rs:594-617`) are lifted into a shared
      location consumed by both hover and inlay hints. Hover output
      remains unchanged.
- [ ] Tests cover: single-module call site, fan-out call site with
      uniform shapes, fan-out with diverging shapes, indexed-port
      range rendering.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

`PatchReferences` is sufficient as-is — no new fields needed.
`call_sites` already groups `FlatNodeRef`s by authored span, and each
flat module carries its `shape`. The work is wiring + a shared
shape-render helper.

VS Code defaults `editor.inlayHints.enabled` to `on`; no client-side
work expected beyond re-bundling the LSP binary.
