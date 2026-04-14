---
id: "0416"
title: Render structured diagnostics in patches-player
priority: medium
created: 2026-04-14
epic: E076
depends_on: ["0415"]
---

## Summary

Replace the ad-hoc renderer from 0414 in `patches-player` with a
terminal renderer driven by `RenderedDiagnostic`. Pick and pin the
rendering crate (likely `ariadne`; `codespan-reporting` is the
fallback).

## Crate choice

- **ariadne**: layout engine. Takes labels with spans + colours,
  decides line selection, draws gutter, carets, multi-file sections,
  arrows between related labels. Uses ANSI colour by default,
  plain-text mode available.
- **codespan-reporting**: simpler, rustc-identical style, less
  pretty for multi-label / expansion cases.

Decision should be made during implementation; document it in the
PR description. Ariadne's cross-label arrow rendering is a natural
fit for expansion chains.

## Acceptance criteria

- [ ] `patches-player` depends on the chosen rendering crate.
- [ ] Error path maps `BuildError` →
      `RenderedDiagnostic::from_build_error(err, &source_map)` →
      rendered output on stderr.
- [ ] Terminal output shows:
  - Header line with severity + message.
  - Primary source snippet with file:line:col, the failing line,
    and a caret underline covering the primary range.
  - One related snippet per expansion entry, each labelled
    "expanded from here".
- [ ] Colour disabled when stderr is not a TTY (respect `NO_COLOR`
      env var).
- [ ] The ad-hoc renderer from 0414 is removed.
- [ ] Golden-snapshot test of stderr for a known three-level
      expansion failure.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

- Implement a `Cache` adapter over `SourceMap` for ariadne (or
  equivalent for codespan). This adapter is small and belongs in
  `patches-player` — not in `patches-diagnostics`, which must stay
  renderer-agnostic.
- Line/column resolution: compute on demand from `SourceMap`'s
  stored text. No need to cache a line index unless profiling
  shows it matters.

## Risks

- ariadne's output order for labels across multiple files can be
  surprising; test with cross-file expansion fixtures.
- `NO_COLOR` handling: confirm behaviour on Windows CMD and
  common CI terminals.
