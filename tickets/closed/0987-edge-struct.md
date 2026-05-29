---
id: "0987"
title: Replace the seven-tuple edge representation with a named Edge struct
priority: medium
created: 2026-05-29
---

## Summary

ADR 0081 named "sub-stages exchange bare tuples" as a root cause of the 0974
class of bugs. E160 introduced typed bundles for stage outputs, but the **edge
representation itself stayed a 7-arity bare tuple**:

```rust
(NodeId, &'static str, usize, NodeId, &'static str, usize, CableMap)
```

The type appears literally in `validate_fused_invariant`,
`validate_scratch_fused_consistency`, `classify_producer_ports`,
`compute_order_with_fusion`, aliased as `EdgeList` in graph_index, and is
destructured by position (`(_, _, _, to, in_name, in_idx, _)`) at every read
site. Positional destructuring of an opaque tuple is exactly the substrate
0974 rode on.

Replace with a named struct:

```rust
pub struct Edge {
    pub from: NodeId,
    pub out_name: &'static str,
    pub out_idx: usize,
    pub to: NodeId,
    pub in_name: &'static str,
    pub in_idx: usize,
    pub map: CableMap,
}
```

## Acceptance criteria

- [ ] Introduce `Edge` in `patches-planner/src/state/graph_index.rs` (next to
      the `EdgeList` alias). `EdgeList` becomes `Vec<Edge>`.
- [ ] `ModuleGraph::edge_list` returns `Vec<Edge>` (or a new
      `edges()` accessor does), or the planner converts at the boundary —
      whichever keeps `patches-core` unchanged if it does not currently expose
      tuples publicly. Document the call-site shim if used.
- [ ] All planner-side consumers read `edge.from`, `edge.to`, etc. — no
      positional destructuring of edge fields.
- [ ] A grep audit of `patches-planner/src/` shows zero instances of
      `(NodeId, &'static str, usize, NodeId, &'static str, usize,` (the start
      of the seven-tuple).
- [ ] Tests touching edges (graph_index, alloc, state/tests, builder/tests)
      build `Edge` values directly.
- [ ] Audio goldens bit-identical; `just push` green.

## Notes

Part of epic **E162**. Pure rename / shape change — no logic touched. If
`patches-core::ModuleGraph::edge_list` needs to change shape, gate that with a
minimal `cargo build -p patches-core` check; nothing downstream depends on the
tuple form. The grep audit criterion is the structural guarantee that 0974's
positional-destructure substrate is gone.
