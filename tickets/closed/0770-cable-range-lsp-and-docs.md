---
id: "0770"
title: LSP hover and docs for cable range expressions
priority: low
created: 2026-04-30
epics: ["E128"]
adrs: ["0062"]
---

## Summary

LSP hover on a range-mapped cable arrow shows the resolved endpoints
(after pitch-family unification and `<param>` substitution where
possible). Update the DSL reference manual with examples.

## Acceptance criteria

- [ ] `patches-lsp` hover provider recognises `uni`/`bi` arrows and
      renders `lo → hi` with units (e.g. `0 V/oct → 6.91 V/oct`,
      `0.2 → 0.8`). For unresolved `<param>` endpoints, show the
      param name with a "param" tag.
- [ ] `docs/src/dsl-reference.md` cable-scale section gains `uni`
      and `bi` subsections with at least:
      - normalized knob → cutoff range example
      - bipolar LFO → frequency range with note literals
      - cross-family rejection example with the diagnostic
- [ ] `docs/src/SUMMARY.md` unchanged (no new pages).
- [ ] `cargo test -p patches-lsp` and `cargo clippy` pass.

## Notes

Reference: [ADR 0062](../../adr/0062-cable-range-expressions.md).
Last ticket in epic [E128](../../epics/open/E128-cable-range-expressions.md);
closes the epic.
