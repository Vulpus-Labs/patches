---
id: "0827"
title: Flatten patches-player tui.rs (1970 LOC, 13 nesting hits)
priority: low
created: 2026-05-06
---

## Summary

[patches-player/src/tui.rs](../../patches-player/src/tui.rs) is 1970 raw
LOC and produces 13 `excessive_nesting` warnings, mostly clustered around
lines 1565–1642 (a render/event match-arm forest) and a separate hit at
1302.

Two complementary moves:

1. Split the file by concern — input handling, layout/render, state
   reducers — into a `tui/` module. Even mechanical extraction makes the
   nesting tractable.
2. Within the deepest match arms, replace nested `if let` ladders with
   `let … else` early returns or extracted helpers per arm.

This is the player binary, not a library hot path, so the bar is
readability rather than perf.

## Acceptance criteria

- [ ] `tui.rs` (or its successor module) has no file > 800 LOC
- [ ] No `excessive_nesting` warnings from the TUI code
- [ ] `patches-player` builds and runs; manual smoke (`patches-player
      examples/shimmer.patches`) shows TUI behaves identically
- [ ] `just commit -p patches-player` clean

## Notes

Low priority: pure readability, no correctness or perf concern. Pick up
when touching tui.rs for another reason.
