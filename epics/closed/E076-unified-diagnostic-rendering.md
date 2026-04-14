---
id: "E076"
title: Unified diagnostic rendering
created: 2026-04-14
depends_on: ["E075"]
tickets: ["0415", "0416", "0417"]
---

## Summary

E075 gives us `Provenance` on flat nodes and `origin` on `BuildError`;
ticket 0414 lands a minimal rendering path in player, LSP, and CLAP.
This epic lifts that rendering into a shared, structured form so all
three consumers present diagnostics with the same information density,
and the CLAP host can show caret-and-underline annotations natively in
its GUI rather than plain text.

The shape:

- A new `patches-diagnostics` crate exposes a `RenderedDiagnostic`
  struct: header (severity, code, message), a primary source snippet
  with highlighted byte ranges, and zero-or-more related snippets
  (the expansion chain + any other related info). It is a *data*
  structure, not a formatter.
- `patches-player` renders `RenderedDiagnostic` to the terminal via
  `ariadne` (or `codespan-reporting`) — whichever is picked when
  0416 lands.
- `patches-lsp` continues to emit its own LSP `Diagnostic` +
  `DiagnosticRelatedInformation`. It shares `Provenance` +
  `SourceMap` inputs but has no use for the rendered form —
  included for completeness only if common extraction helpers live
  in `patches-diagnostics`.
- `patches-clap` consumes `RenderedDiagnostic` and renders it into
  the plugin's GUI error surface as styled text runs (colour on
  highlighted ranges, dim on context lines), so authors inside a
  host see the same annotated view they'd see in a terminal.

## Acceptance criteria

- [ ] `patches-diagnostics` crate exists with `RenderedDiagnostic`,
      `Snippet`, `Highlight` types documented.
- [ ] `patches-player` terminal output uses the chosen rendering crate
      driven by `RenderedDiagnostic`.
- [ ] `patches-clap` GUI shows annotated source snippets (primary +
      expansion chain) with highlighted ranges, replacing the plain
      text window.
- [ ] The old ad-hoc renderer from 0414 is removed; player and CLAP
      go through the shared path.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Tickets

| ID   | Title                                                |
|------|------------------------------------------------------|
| 0415 | Introduce patches-diagnostics crate (structured form)|
| 0416 | Render structured diagnostics in patches-player      |
| 0417 | Render structured diagnostics in patches-clap GUI    |
