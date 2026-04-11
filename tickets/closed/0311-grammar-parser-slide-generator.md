---
id: "0311"
title: "Grammar + parser: slide() generator sugar"
priority: medium
created: 2026-04-11
---

## Summary

Add grammar and parser support for the `slide(n, start, end)` generator
syntax inside pattern channel rows. This is expand-time sugar that
produces a sequence of `n` slide steps interpolating cv1 from `start` to
`end`.

## Acceptance criteria

- [ ] Grammar rule `slide_generator` matching `slide(int, float, float)`
- [ ] Expansion produces `n` slide steps, e.g. `slide(4, 0.0, 1.0)` →
      four steps: `0.0>0.25`, `0.25>0.5`, `0.5>0.75`, `0.75>1.0`
- [ ] cv2 carries over from the preceding step (not set by the generator)
- [ ] Slide generators can appear inline in channel rows alongside
      regular steps
- [ ] Unit tests: basic slide, slide at start of row, slide mid-row,
      slide with non-zero start
- [ ] `cargo test -p patches-dsl` passes
- [ ] `cargo clippy -p patches-dsl` clean

## Notes

The expansion happens at parse time (or expand time) — by the time the
data reaches the interpreter, slide generators have been replaced with
concrete step sequences. This is analogous to how template parameters are
resolved during expansion.

Epic: E057
ADR: 0029
