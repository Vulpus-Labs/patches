---
id: "0373"
title: Fix mutable aliasing UB in CLAP plugin_activate
priority: high
created: 2026-04-13
---

## Summary

In `patches-clap/src/plugin.rs` lines 247–253, `plugin_activate` creates a
`&mut` reference to the plugin data via `plugin_mut`, then calls
`plugin_deactivate` which internally calls `plugin_mut` again — producing two
simultaneous `&mut` references to the same data. This is undefined behaviour.

## Acceptance criteria

- [ ] Restructure `plugin_activate` so only one `&mut` exists at a time
- [ ] Remove the dead `let _ = p;` reborrow
- [ ] No clippy warnings introduced

## Notes

The fix is to drop the outer `p` borrow before calling `plugin_deactivate`,
or restructure the control flow so the deactivate branch doesn't need the
outer reference. A simple approach: check `p.processor.is_some()` into a
bool, drop `p`, then conditionally call `plugin_deactivate`.
