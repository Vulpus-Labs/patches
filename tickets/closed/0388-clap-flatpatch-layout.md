---
id: "0388"
title: Switch clap GUI to FlatPatch-driven layout, delete PatchSnapshot
priority: medium
created: 2026-04-13
---

## Summary

`patches-clap/src/gui.rs` defines `PatchSnapshot`/`SnapshotNode`/`SnapshotEdge`
built from `ModuleGraph`. SVG rendering (0389) and downstream consumers
(LSP 0390, CLI 0392) can work directly from the `FlatPatch` produced by
`patches-dsl::expand`, which already carries module ids, type names, and
port-level connections. This is simpler, works on partially-invalid patches
(unknown module types, type errors), and avoids pulling `patches-interpreter`
into the render chain.

Delete `PatchSnapshot` and switch the clap GUI to build its layout input
directly from a stashed `FlatPatch`.

## Scope

- Delete `PatchSnapshot`, `SnapshotNode`, `SnapshotEdge`, `port_label`,
  `PatchSnapshot::from_graph` from `patches-clap/src/gui.rs`.
- `GuiState.patch_snapshot: Option<PatchSnapshot>` becomes
  `GuiState.flat_patch: Option<patches_dsl::FlatPatch>`.
- `patches-clap/src/plugin.rs::compile_and_push_plan` already calls
  `patches_dsl::expand` — stash the resulting `FlatPatch` into `GuiState`
  instead of building a snapshot from the `ModuleGraph`.
- `patches-clap/src/gui_vizia.rs` consumes `FlatPatch`; convert to
  `patches_layout::LayoutNode`/`LayoutEdge` via the shared adapter exposed
  from `patches-svg` (ticket 0389).
- Show only ports that appear in at least one connection (match current
  behaviour). Insertion order of connections determines port row order.

## Acceptance criteria

- [ ] `PatchSnapshot` and related types removed from workspace.
- [ ] Clap GUI renders identically from `FlatPatch`.
- [ ] No `ModuleGraph` plumbing remains for GUI snapshot purposes.
- [ ] `cargo clippy` clean.

## Notes

- No new shared crate or module. Adapter lives in `patches-svg`
  (ticket 0389) and is reused by clap; LSP and CLI go through
  `patches-svg::render_svg(&FlatPatch, ...)` which calls the adapter
  internally.
- `patches-interpreter` stays out of the render path.
