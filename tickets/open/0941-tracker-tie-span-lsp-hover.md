---
id: "0941"
title: Syntax corpus + LSP hover for tie-spread roll
priority: low
created: 2026-05-19
epic: E152
depends_on: ["0939"]
---

## Summary

Add a `patches-lsp/tests/syntax_corpus/` entry for tie-spread roll
patterns and surface the disambiguated tie meaning in LSP hover.

Per syntax-corpus policy, any change to how a syntactic construct is
*interpreted* — even without a grammar change — warrants a corpus
entry so future grammar/lower passes don't silently regress it.

## Acceptance criteria

- [ ] New corpus file under `patches-lsp/tests/syntax_corpus/`
      containing at least:
      ```text
      pattern p {
        kick: x*3 ~      x*5 ~ ~     x*3
        snare: . x ~     . . x*2 ~   . .
      }
      ```
      with the expected lower output capturing `repeat`, `repeat_span`,
      and `absorbed_by_roll` annotations.
- [ ] LSP hover on a tie cell distinguishes the two cases in its hover
      text:
  - tie after plain step → "Tie: hold gate, no trigger. cv1/cv2
    carry."
  - tie absorbed by `*N` roll → "Roll continuation: extends *N
    spread from previous anchor across this tick."
- [ ] LSP hover on a `*N` anchor with `span > 1` shows the derived
      span: "Repeat 3 over 2 ticks (anchor + 1 tie)."
- [ ] `just inner -p patches-lsp` green; corpus regenerated and
      committed.

## Notes

- This ticket only fires after 0939 because the LSP hover needs the
  row-build annotations to know which interpretation applies. If the
  LSP runs a separate lower pass that doesn't share the row-build
  code, factor the span-derivation helper out of 0939's row-build into
  a small shared function.
