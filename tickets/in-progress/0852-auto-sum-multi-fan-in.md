---
id: "0852"
title: Auto-sum multiple connections targeting the same input port
priority: medium
created: 2026-05-09
---

## Summary

Allow multiple connections to fan into the same input port through the DSL
pipeline. At the registry-aware interpretation stage (`patches-interpreter`
descriptor bind), validate that all sources share the target's cable kind
(mono / poly / stereo) and synthesize a `Sum` / `PolySum` / `StereoSum`
module of `channels = N` to merge them. Per-source `CableMap` (scale /
offset / clip) is preserved on the source→sum edges; the sum→target edge is
identity.

Replaces the prior `BindErrorCode::DuplicateInputConnection` rejection: the
runtime graph builder still enforces single-source per input (RT0001), and
the auto-sum rewrite is what makes that invariant hold.

## Acceptance criteria

- [x] `StereoSum` module exists in `patches-modules` and is registered in
      the default registry.
- [x] Multi-fan-in into a single input port produces a clean `BoundPatch`
      with a synthesized `__autosum_*` Sum-family node and rewritten edges
      (no `BN0009`).
- [x] Source `CableMap` survives the rewrite on source→sum edges.
- [x] Heterogeneous fan-in (e.g. mono + poly into the same target) emits
      `BindErrorCode::HeterogeneousFanIn` (`BN0014`); the per-edge
      `CableKindMismatch` already covers most cases (target kind disagrees
      with at least one source), so `BN0014` only fires when both edges
      independently bind successfully but disagree on poly layout.
- [x] LSP tests updated (`BN0009` retired in span checks).
- [x] Inner-tier (`just inner -p patches-interpreter`) passes.

## Notes

- Implementation lives in
  `patches-interpreter/src/descriptor_bind/fan_in.rs`. The pass runs after
  per-edge `bind_connection` and before `port_refs` are committed.
- Sum kind selection is by *source* kind (which is uniform after the
  homogeneity check). Mono sources targeting a stereo input still travel
  through `Sum` (mono); the existing single-edge mono→stereo broadcast on
  `StereoInput` handles the final widening on the sum→target edge.
- Sum modules are Audio-only. Trigger / MIDI / Transport poly layouts
  cannot fan in; bind emits `BN0014` for those.
- New `BindErrorCode` codes:
  - `HeterogeneousFanIn` → `BN0014`
  - `AutoSumModuleMissing` → `BN0015` (registry lacks the synthesized
    `Sum` / `PolySum` / `StereoSum` module — only fires for non-default
    registries).
- `DuplicateInputConnection` (`BN0009`) is retained as an enum variant
  for ABI stability but is no longer emitted by bind.
