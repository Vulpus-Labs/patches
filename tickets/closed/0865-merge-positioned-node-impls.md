---
id: "0865"
title: Merge two `impl PositionedNode` blocks
priority: low
created: 2026-05-10
epic: E144
---

## Summary

[patches-svg/src/layout.rs:116](patches-svg/src/layout.rs#L116) (added
0857, `is_summed_input`) and
[patches-svg/src/layout.rs:124](patches-svg/src/layout.rs#L124)
(pre-existing `port_y`, `input_x`, `output_x`) are two `impl
PositionedNode` blocks back-to-back. Merge.

## Acceptance criteria

- [ ] Single `impl PositionedNode` block.
- [ ] `just push` clean.
