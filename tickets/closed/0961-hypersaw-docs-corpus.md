---
id: "0961"
title: Docs + DSL corpus for HyperSaw / PolyHyperSaw
priority: low
created: 2026-05-27
depends_on: ["0959", "0960"]
---

## Summary

Surface the new oscillators in the manual and the DSL syntax corpus once the
modules land (0959/0960). The module reference is generated from the doc
comments (standard form), so this ticket is the manual prose + at least one
worked patch example + a corpus entry exercising the modules.

## Acceptance criteria

- [x] `docs/src/modules/oscillators.md` reference includes `HyperSaw` and
      `PolyHyperSaw` (Inputs/Outputs/Parameters tables, port names matching the
      descriptors).
- [x] A manual passage explaining the supersaw concept: spread (detune), density
      (copy fade-in), mix (centre↔side), and why FM is vibrato-rate (links ADR
      0078). — "Supersaw" section in oscillators.md.
- [x] Example `.patches` using `PolyHyperSaw` into filter + amp, referenced from
      the manual. — [`examples/synths/hypersaw_lead.patches`](../../examples/synths/hypersaw_lead.patches);
      builds end to end (`hypersaw_lead_example_builds` in `dsl_pipeline.rs`).
- [x] `patches-lsp/tests/syntax_corpus/hypersaw.corpus` instantiates both modules
      with params + CV wiring (pest/tree-sitter agree, snippets expand).
- [x] Tables aligned (`tools/align-tables.py`); MD060 clean.
- [x] `just commit` green for touched crates. (mdbook HTML renders; the optional
      `pandoc` backend isn't installed locally — pre-existing, unrelated.)

## Notes

- No code behaviour here — docs, example patch, corpus only.
- If `just smoke` renders/auditions example patches, add the new example to that
  set so it gets exercised.
