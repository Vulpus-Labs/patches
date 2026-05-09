---
id: "0854"
title: Builder accepts multi-edge inputs; retire fan-in synthesised-Sum
priority: medium
created: 2026-05-09
closed: 2026-05-09
status: superseded
epic: E142
---

## Status: superseded

ADR 0071 rejected. Auto-Sum (ticket 0852) kept; fusion (ADR 0072) covers
the delay concern. See ADR 0071 §Resolution.

## Summary

Second step of ADR 0071. With the port shape from ticket 0853 in place,
let the graph builder accept many edges per input port and populate the
target port's `sources` slice from the resulting edge list. Retire the
`fan_in.rs` synthesised-Sum rewrite in `patches-interpreter` along with
its three `BindErrorCode` variants — they exist solely to paper over a
limitation the builder no longer has.

## Acceptance criteria

- [ ] `ModuleGraph::edges` storage moves from
      `HashMap<(NodeId, &str, usize), Edge>` to
      `HashMap<(NodeId, &str, usize), Vec<Edge>>`. Iteration order is
      deterministic (insertion order — the cable builder's expected
      input).
- [ ] `ModuleGraph::connect_with_map` appends to the per-input edge
      list instead of returning `GraphError::InputAlreadyConnected`.
      The `InputAlreadyConnected` variant is removed.
- [ ] Cable builder allocates one cable per producing output (unchanged)
      and packs each input's edges into that input's `sources` slice
      in connect order. Per-edge `scale`, `offset`, `clip`, and (stereo
      only) the `broadcast_from_mono` decision land on the corresponding
      `Source` record.
- [ ] `patches-interpreter::descriptor_bind::fan_in` is deleted along
      with its module declaration in `mod.rs`. Bind passes resolved
      multi-edge connections straight through to the builder.
- [ ] `BindErrorCode::DuplicateInputConnection` (`BN0009`),
      `HeterogeneousFanIn` (`BN0014`), and `AutoSumModuleMissing`
      (`BN0015`) are removed from the enum. Per-edge kind / poly-layout
      / mono-layout validation in `bind_connection` is unchanged.
- [ ] Tests previously asserting `BN0009` / `BN0014` /
      `InputAlreadyConnected` are rewritten to assert that the multi-edge
      patch builds cleanly and the resulting graph contains exactly the
      user's modules (no `__autosum_*` synthesised node, no leftover
      Sum-family node from this ticket either — they're still alive
      until 0855).
- [ ] LSP `spans.rs` test for fan-in (currently asserts no `BN0009` /
      `BN0014`) updated to reflect the retired codes; no other LSP test
      regressions.
- [ ] `just inner -p patches-core -p patches-engine -p patches-interpreter
      -p patches-lsp` green.

## Out of scope

- `Sum` / `PolySum` / `StereoSum` deletion — that's ticket 0855.
- Adjusting `SmallVec` inline capacity. `[Source; 1]` is the working
  default; bumping is a follow-up if profiling demands it.

## Notes

- The runtime invariant ModuleGraph wants — every input has at least
  one edge or is wired to the read-sink, every cable has a producer —
  is unchanged. The only difference is "every input has *at most one*
  edge" relaxes to "every input has *at least one* edge".
- Per-edge clip applies before summation in the read path (set in
  ticket 0853). If a future ticket wants pre-clip / post-clip toggle
  behaviour it'll need a separate flag; out of scope here.
- `Provenance` for synthesised `__autosum_*` nodes goes away with
  `fan_in.rs`. No diagnostics regression: per-edge bind errors point
  at the offending edge as they always did.
- The graph builder's deterministic iteration order is load-bearing
  for snapshot tests. `Vec<Edge>` preserves insertion order trivially.
