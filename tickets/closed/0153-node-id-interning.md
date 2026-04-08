---
id: "0153"
title: Intern NodeId strings via Arc<str>
epic: E026
priority: low
created: 2026-03-20
---

## Summary

`NodeId` in `patches-core/src/graphs/graph.rs` wraps a `String`. Every `.clone()` allocates a new heap string. `NodeId` is used heavily as a `HashMap` key and is cloned throughout graph construction, planner diffing, and DSL expansion. Switching to `Arc<str>` makes clones O(1) (pointer bump) and reduces allocator pressure during patch loads.

## Acceptance criteria

- [ ] `NodeId` is changed to wrap `Arc<str>` instead of `String`.
- [ ] Construction from `&str` / `String` still works via `From<&str>` and `From<String>` impls.
- [ ] `Display` and `Debug` impls are preserved.
- [ ] All compilation errors caused by the change are fixed across the workspace.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

This ticket was implemented and then reverted. `NodeId` genuinely needs to be owned in multiple places simultaneously (as HashMap keys in `nodes` and `edges`, and as `from`/`to` fields in `Edge` structs), so borrowing can't eliminate the copies. The remaining question is `String` vs `Arc<str>`.

For the actual content (short strings like `"osc"`, `"out"`, `"sine"`), `Arc<str>` is not a clear win: it trades a small heap allocation + memcpy (String clone) for an atomic increment, but carries a refcount prefix and cache overhead. The crossover where `Arc` wins requires many clones per NodeId, which doesn't occur in a typical patch (a handful of nodes, each cloned a handful of times during graph construction).

**This ticket should be closed as won't-do** unless profiling shows NodeId cloning as a measurable bottleneck.

`Arc<str>` has the same `Hash` and `Eq` behaviour as `String` for the same bytes, so `HashMap` lookups are unaffected. The `Ord`/`PartialOrd` impls should also behave identically.
