---
id: "0417"
title: Render structured diagnostics in patches-clap GUI
priority: medium
created: 2026-04-14
epic: E076
depends_on: ["0415"]
---

## Summary

Replace the CLAP host's plain-text error window with a GUI view that
renders `RenderedDiagnostic` natively — source snippets with
highlighted byte ranges, expansion chain as a stack of related
snippets.

## Acceptance criteria

- [ ] CLAP error surface consumes `RenderedDiagnostic`, not a
      pre-formatted string.
- [ ] Primary and each related snippet render as:
  - A header line: `<file>:<line>:<col>` (+ label).
  - The failing line(s) with the highlighted byte range drawn in
    an accent colour; surrounding context dimmed or neutral.
- [ ] Expansion chain snippets labelled "expanded from here",
      visually distinguished from the primary.
- [ ] Layout degrades gracefully for long lines (horizontal scroll
      or wrap — pick whichever the existing vizia text surface
      supports).
- [ ] The ad-hoc renderer from 0414 in `patches-clap` is removed.
- [ ] Manual smoke test with a known three-level expansion failure
      loaded into a CLAP host (e.g. Bitwig, Reaper) — screenshot
      attached to the PR.

## Notes

- GUI stack is vizia/baseview (ADR 0026). The renderer is a small
  widget tree: one snippet widget per `Snippet`, stacked
  vertically, each composed of a header label and a pre-formatted
  code view.
- Line/column resolution: share the helper from 0416 where
  practical — lift it into `patches-diagnostics` as a utility if
  both frontends want it (this is the one piece of "rendering-
  adjacent" logic it's safe to host there, since it returns raw
  `(line, col)`, not styled output).
- Colours: pick from the existing plugin theme rather than ANSI
  defaults. Error = accent red, expansion = muted foreground.

## Risks

- vizia's text-with-inline-highlights support may be limited;
  worst case, render each line as a row of styled spans with
  manual positioning. Scope check early.
- Font metric differences between host chrome and plugin GUI —
  if line wrapping matters, use the plugin's own text layout, not
  terminal-style column counts.
