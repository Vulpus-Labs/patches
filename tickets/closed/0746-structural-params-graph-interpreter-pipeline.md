---
id: "0746"
title: Thread structural params from DSL through graph and interpreter into the planner
priority: high
created: 2026-04-29
epic: "E126"
adrs: ["0060"]
depends_on: ["0734", "0737", "0740"]
---

## Summary

Carry `StructuralParams` end-to-end on the native (non-FFI) construction
path so DSL-declared structural values (e.g. `convolution_reverb`'s
`ir_path`) reach `Module::prepare` for real. Today the trait signature
takes `&StructuralParams` (0734), conv-reverb reads `ir_path` inside
`prepare` (0737), and the planner can diff and rebuild on a structural
edit (0740) — but `Registry::create` / `Module::build` always pass
`StructuralParams::new()`, so the value the DSL specifies never arrives.

Scope is the propagation pipeline only. The planner-side diff/rebuild
behaviour landed in 0740; this ticket fills in the upstream half so
0740's acceptance test can be written against the DSL surface rather
than a hand-built graph.

## Acceptance criteria

- [ ] `graph::Node` carries a `StructuralParams` alongside its
      `ParameterMap`. `ModuleGraph::add_module` accepts it and the
      builder reads it.
- [ ] Interpreter `convert_params` splits incoming DSL parameter pairs
      into realtime vs structural by descriptor, populating a
      `ParameterMap` and a `StructuralParams` from the same input.
      DSL string literals (and the `file("…")` desugaring from 0737)
      route into `StructuralValue::String`.
- [ ] `BoundPatch` / `ResolvedModule` carry structural alongside the
      realtime `ParameterMap`. `build_from_bound` propagates both into
      the graph node.
- [ ] `ModuleBuilder::build` and `Registry::create` take
      `&StructuralParams`. `Module::build` forwards it to
      `Module::prepare` instead of constructing an empty carrier.
      Remove the transitional empty-carrier branch in
      `patches-core/src/modules/module.rs`.
- [ ] Planner builder pulls structural from the graph node and passes
      it via the registry on `Install` (replacing the empty carrier
      added by 0740 at the planner↔registry seam).
- [ ] Planner `NodeState.structural` records the structural snapshot
      used at install; the diff in `classify_nodes` (added by 0740)
      now compares against the node's actual structural rather than
      against an always-empty baseline.
- [ ] Integration test: load a `.patches` file declaring
      `convolution_reverb { ir_path = file("…") }`, build, and verify
      the convolver loaded the IR (e.g. observe a non-empty pre-FFT
      cache, or a non-trivial impulse response when ticked).
- [ ] Integration test: hot-reload that patch with a different
      `ir_path` and observe the planner mints a fresh instance
      (matches 0740's structural-rebuild path end-to-end).
- [ ] `cargo test` and `cargo clippy` pass on the inner-loop subset.

## Notes

`ParameterValue` deliberately has no `String` variant; structural
strings live exclusively in `StructuralValue` per ADR 0060. Resist the
temptation to merge the two carriers — the realtime path (`ParamFrame`,
audio thread) must remain non-allocating.

FFI is unaffected: 0739 already pipes structural through the plugin
ABI. The `DylibModuleBuilder::build` adapter just needs the new
`&StructuralParams` argument plumbed into its existing
`pack_structural` call site.

The DSL surface for `file("…")` desugars to a structural string param
post-0737; this ticket only wires the runtime side, no grammar change.

## Status

Implemented (2026-04-29):

- `graph::Node` carries `structural: StructuralParams`. New
  `ModuleGraph::add_module_with_structural` is the interpreter's entry
  point; `add_module` stays as a thin wrapper passing an empty carrier
  so test fixtures across the workspace are unaffected.
- Interpreter `convert_params` now returns
  `(ParameterMap, StructuralParams)`. Realtime pairs are routed by
  `descriptor.realtime_params`; pairs whose name appears in
  `descriptor.structural_params` are converted into `StructuralValue`s
  (Bool/Int/Float scalars, plus File/String → `StructuralValue::String`
  with extension validation and `base_dir` resolution).
- `ResolvedModule` gains a `structural` field; `bind_module` populates
  it and `build_from_bound` threads it into the graph node via
  `add_module_with_structural`.
- Planner `make_decisions` / `classify_nodes` drop the
  `structural_by_node` argument and read directly from
  `node.structural`. `PatchBuilder::build_patch_with_structural` and
  `Planner::build_with_structural` are removed; the canonical
  `build_patch` / `Planner::build` paths now thread structural through
  to `Module::prepare` on `Install`.
- `ConvolutionReverb` exposes `prepared_with_ir_path()` as a sticky
  observation hook that survives `apply_unpacked_params`.
- New integration test
  `patches-integration-tests/tests/structural_pipeline.rs` loads a
  `.patches` source declaring `ConvReverb { ir_path: file("…") }`,
  asserts the IR reached `prepare`, and rebuilds with a different path
  to confirm the planner mints a fresh instance (matches 0740's
  structural-rebuild path end-to-end).
