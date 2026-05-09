---
id: E142
title: Multi-source input ports (ADR 0071)
status: superseded
created: 2026-05-09
closed: 2026-05-09
---

## Status: superseded

ADR 0071 rejected. Auto-Sum (ticket 0852) kept; fusion (ADR 0072 / E141)
covers the cable-delay concern that motivated the multi-source rewrite.
Tickets 0853, 0854, 0855, 0856 closed superseded. See ADR 0071 §Resolution.

## Goal

Implement ADR 0071. Replace the synthesized-Sum rewrite from ticket 0852
with native multi-source input ports: every input carries an inline
collection of `Source` records (cable index + affine map + flags), and
`pool.read_*` iterates and sums them. Fan-in is a property of the port,
not a synthesised graph node.

After this epic:

- The graph the engine runs is the graph the user wrote — no
  `__autosum_*` nodes, no extra cable hops, no extra `process()` frames.
- `Sum` / `PolySum` / `StereoSum` retire from `patches-modules` and the
  default registry. The three modules existed to express what input
  ports now express directly.
- Per-edge kind / layout validation is unchanged. The new degree of
  freedom is that a single input may have multiple validated edges
  pointing at it.

## Scope

In:

- `Source` struct + `SmallVec<[Source; 1]>` on `MonoInput`, `PolyInput`,
  `StereoInput`. Single-edge inputs stay inline; multi-edge inputs spill
  to a heap buffer fixed at build time.
- `read_mono` / `read_poly` / `read_stereo` iterate sources, applying
  per-source `scale`, `offset`, `clip`, and (stereo only)
  `broadcast_from_mono`, summing.
- `ModuleGraph::edges` keys store `Vec<Edge>` per input; cable builder
  populates the port's `sources` slice from the edge list.
  `connect_with_map` appends instead of returning
  `GraphError::InputAlreadyConnected`.
- `patches-interpreter::descriptor_bind` drops the `fan_in.rs`
  synthesised-Sum rewrite and the `DuplicateInputConnection` /
  `HeterogeneousFanIn` / `AutoSumModuleMissing` `BindErrorCode` variants.
- `Sum`, `PolySum`, `StereoSum` (and their tests) deleted from
  `patches-modules`; removed from `default_registry`.
- LSP syntax corpus, SVG fixtures, mdBook module reference, and any
  in-tree `.patches` fixture that named a `Sum`-family module migrated
  to direct fan-in.

Out:

- Output ports stay single-cable. Multi-output is a separate question
  (none of today's modules expose it).
- The cable allocator and cable-pool layout are unchanged. One cable per
  producing output; many readers per cable is already supported.
- Cable-map evaluation order: per-source clip applies before summation,
  matching today's `Sum`-after-clip semantics.

## Tickets

- 0853 — Multi-source input port shape (`Source` + `SmallVec<[Source; 1]>`,
  read-helper iteration, harness/test_support migration). Builder still
  emits at most one edge per input — no user-visible change.
- 0854 — Builder + cable builder accept multi-edge inputs;
  `connect_with_map` appends; retire `fan_in.rs` and the dup-input /
  heterogeneous-fan-in / auto-sum-missing `BindErrorCode` variants.
- 0855 — Delete `Sum` / `PolySum` / `StereoSum` modules + tests;
  remove from `default_registry`; migrate LSP corpus, SVG fixtures,
  mdBook module reference, and any `.patches` fixtures.

## Notes

- This epic supersedes ticket 0852. 0852 ships the user-facing fan-in
  behaviour through a synthesised-Sum rewrite; E142 replaces that
  implementation with the structurally-correct one. If 0852 lands first,
  ticket 0854 retires its `fan_in.rs`; if E142 lands directly, 0852 was
  a self-contained warm-up that shipped the right behaviour without the
  right shape.
- The `SmallVec<[Source; 1]>` choice gates one design knob: how many
  sources can fit inline before the heap kicks in. `1` is the realistic
  inflexion point — almost every input is single-source, every measured
  fan-in case in the corpus is ≤4. Bumping the inline budget to `[Source; 2]`
  costs ~32 extra bytes per input and is a one-line change if profiling
  later shows two-edge fan-in is hot.
- Per-edge `MonoLayout` (Audio / Trigger) heterogeneity is rejected at
  bind today and will continue to be: bind validates each source
  independently against the target, so a Trigger source fanning into
  an Audio target fails as it does now. There is no "promote on read"
  cross-layout coercion.
