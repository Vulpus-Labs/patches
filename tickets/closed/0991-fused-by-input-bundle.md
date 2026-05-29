---
id: "0991"
title: Per-input fused-flag bundle; producer-port key newtype
priority: medium
created: 2026-05-29
---

## Summary

Two derived-fact shapes are constructed ad-hoc inside `build_draft` today,
even though they are pure functions of frozen prior bundles:

1. `fused_by_input: HashMap<(NodeId, &'static str, usize), bool>` — built
   inline at the top of `build_draft` from `topology.cable_fused` + edges, so
   each consumed input port has O(1) access to its fused flag.
2. `to_zero_set: HashSet<usize>` — built from `layout.alloc.to_zero` for
   poly/stereo output-port zeroing.

(1) is the more interesting case: it's an alternative view of
`Topology::cable_fused` keyed by *consumer* input port rather than edge
index. It belongs alongside `Topology` (or in a derived `FusionByInput`
bundle), not in the consumer that reads it. ADR 0081 prohibits exactly this
"IR construction inside the consumer."

Additionally, two key shapes in the IR layer are anonymous tuples that
encode the same fact (producer port at slice position):

- `PortClassification::producer_port_cycle: HashMap<(NodeId, usize), bool>`
- `BufferAllocation::output_buf: HashMap<(NodeId, usize), usize>`
- `BufferAllocState::output_buf` (same shape)

Introduce a named `ProducerPortKey { node: NodeId, slice_pos: usize }` and use
it in all three places. Likewise, name the consumer-input key as `InputPortKey`
or similar.

## Acceptance criteria

- [ ] `Topology` exposes either a `fused_by_input` field, computed in
      `Topology::build`, or a new sibling bundle `FusionByInput { map }`
      derived once from `(Topology, edges)`. `build_draft` reads it
      directly — does not construct it.
- [ ] If a separate bundle is chosen, it follows the same
      `IR_prev -> IR_next` shape as other stages (`Result` not required
      since this derivation is infallible).
- [ ] `ProducerPortKey` newtype replaces the anonymous `(NodeId, usize)` key
      in `PortClassification::producer_port_cycle`,
      `BufferAllocation::output_buf`, `BufferAllocState::output_buf`,
      `ResolvedGraph::build`'s `output_buf` parameter, and the alloc helpers'
      `cycle_already_freed` / `key` test helpers.
- [ ] `InputPortKey` (or equivalent) replaces the
      `(NodeId, &'static str, usize)` triple in `fused_by_input`,
      `GraphIndex::connected_inputs` / `connected_outputs`, and
      `InputBufferMap`.
- [ ] Direct unit tests on the new bundle / key shapes, mirroring the 0975
      / 0976 lock-in style.
- [ ] Audio goldens bit-identical; `just push` green.

## Notes

Part of epic **E162**. Independent of 0987 / 0988 / 0989 — runs in parallel.
`to_zero_set` is small enough to leave inline (it's a local view, single use
site); the criterion above is just for `fused_by_input`. The producer-port
key newtype is the structural guarantee that nothing downstream of
`PortClassification` can key by something other than the slice position
(the 0974 root cause, generalised).
