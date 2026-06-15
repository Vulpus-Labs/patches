---
id: "1004"
title: "Retire temporary patches-engine → patches-planner re-exports"
priority: low
created: 2026-06-11
---

## Summary

The E160 kernel carve moved the build/plan types into `patches-planner`,
but `patches-engine/src/lib.rs` keeps a block of `pub use
patches_planner::{...}` re-exports (`build_patch`, `BuildError`,
`ExecutionPlan`, `PatchBuilder`, `Planner`, `PlannerState`, …) marked
*temporary* to ease migration. Downstream crates (notably
`patches-integration-tests`) still import these through `patches_engine`
rather than `patches_planner` directly, so the re-exports can't simply be
deleted yet.

## Acceptance criteria

- [ ] Repoint every downstream import of the re-exported planner symbols
      from `patches_engine::…` to `patches_planner::…`.
- [ ] Delete the temporary re-export block from
      `patches-engine/src/lib.rs`.
- [ ] `just push` green.

## Notes

Surfaced by the 2026-06 doc-drift review (ticket 1001). Mechanical but
touches several call sites; low priority — the re-exports are harmless,
just untidy.
