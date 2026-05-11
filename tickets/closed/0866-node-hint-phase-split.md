---
id: "0866"
title: Split `NodeHint` by enrichment phase
priority: low
created: 2026-05-10
epic: E144
---

## Summary

`patches-svg::layout::NodeHint` is populated in two passes that must
not overwrite each other's fields:

- `flat_to_layout::flat_to_layout_input` writes `summed_input_ports`
  (set when an auto-Sum module is collapsed into the consumer).
- `hints::apply_node_hint` writes `tooltip` and `data_attrs` (source-
  map enrichment).

0857 fixed a regression where the second pass clobbered the first by
switching to in-place mutation, with a comment at the call site
explaining the contract. The `NodeHint` struct itself carries no hint
of this — any new field gets a coin-flip on whether it should be
phase-additive.

Encode the contract in the type. Two natural shapes:

1. Two structs composed on `LayoutNode`:
   `graph_hint: GraphShapeHint` (owns `summed_input_ports`),
   `source_hint: SourceMapHint` (owns `tooltip`, `data_attrs`).
   Each enrichment pass writes its struct. Phase separation enforced
   structurally.
2. A single `NodeHint` with field-level docs marking which pass owns
   which. Cheaper; still relies on reviewer attention.

Prefer (1) — the user's "explicit types and value-evolution rules"
desideratum points there.

## Acceptance criteria

- [ ] `LayoutNode` carries two hint members (or whatever shape
      survives review). Each enrichment pass writes only its own.
- [ ] `apply_node_hint` no longer needs the "additive enrichment"
      comment.
- [ ] Renderer reads both as needed; no behavior change visible in
      SVG output (existing snapshot tests still pass).
- [ ] `just push` clean.
