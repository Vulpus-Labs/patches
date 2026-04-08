---
id: "0156"
title: Store Arc<ModuleDescriptor> in graph nodes to avoid repeated clones
epic: E026
priority: low
created: 2026-03-20
---

## Summary

`ModuleDescriptor` is static data produced by `Module::describe()` and is never mutated after creation. `ModuleGraph::add_node()` in `patches-core/src/graphs/graph.rs` (lines 150-152) clones it into each `Node` struct. For descriptors with many ports or parameters, this is a non-trivial copy that scales with the number of modules in the patch.

Changing `Node::module_descriptor` to `Arc<ModuleDescriptor>` makes `add_node` accept an `Arc` (or wrap internally) so subsequent lookups and planner reads are pointer copies, not deep clones.

## Acceptance criteria

- [ ] `Node::module_descriptor` is changed to `Arc<ModuleDescriptor>`.
- [ ] `ModuleGraph::add_node` accepts `Arc<ModuleDescriptor>` (callers that currently pass owned values should be updated to wrap with `Arc::new`).
- [ ] Planner and other read paths access the descriptor via the `Arc` without needing to clone.
- [ ] All compilation errors across the workspace are fixed.
- [ ] `cargo clippy` clean; `cargo test` passes.

## Notes

This ticket was implemented and then reverted. The Arc approach buys nothing as written: each `add_module` call wraps a freshly-constructed descriptor in a new Arc (refcount 1), so no actual sharing occurs. The apparent `.clone()` overhead in a few test sites was unnecessary — those sites were fixed to borrow `&ModuleDescriptor` directly from the node instead.

`Arc` would only be worthwhile here if the registry held one `Arc<ModuleDescriptor>` per module type and nodes cloned that shared Arc. However, descriptors vary by shape (not just by module type), so a registry-keyed-by-type cache wouldn't help, and keying by (type, shape) would just replicate what the graph already does.

**This ticket should be closed as won't-do** unless a genuine sharing pattern emerges in future.
