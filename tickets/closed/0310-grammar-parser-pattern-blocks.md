---
id: "0310"
title: "Grammar + parser: pattern blocks"
priority: high
created: 2026-04-11
---

## Summary

Add pest grammar rules and parser logic for `pattern` blocks — named,
multi-channel grids of step data. Each channel row has a label followed by
a colon and a sequence of steps.

## Acceptance criteria

- [ ] Grammar rule `pattern_block` as a top-level construct
- [ ] Channel rows: `label: step step step ...`
- [ ] Line continuation via trailing `|`: a row can span multiple lines
- [ ] Step count inferred from the longest channel row; shorter rows
      padded with rests
- [ ] Parser produces `PatternDef` AST nodes (from ticket 0308)
- [ ] `file` rule updated to accept `pattern` blocks (zero or more,
      before or after templates, before the patch block)
- [ ] Unit tests: single-channel pattern, multi-channel pattern,
      continuation lines, uneven row lengths
- [ ] `cargo test -p patches-dsl` passes
- [ ] `cargo clippy -p patches-dsl` clean

## Notes

Pattern blocks are order-independent at the file level — they can appear
before or after templates. The parser collects them into
`File.patterns`.

Channel names within a pattern are local to that pattern. They become
meaningful when a PatternPlayer declares its channels with matching
aliases.

Epic: E057
ADR: 0029
