---
id: "0981"
title: proptest scaffolding for the planner — generators + harness
priority: medium
created: 2026-05-29
closed: 2026-05-29
---

## Done

- `proptest = "1"` added to `patches-planner/Cargo.toml` `[dev-dependencies]`.
- New integration target `patches-planner/tests/properties.rs`.
- Five test modules (`MonoIO`, `MultiOut`, `PolyIO`, `StereoIO`, `MonoSink`)
  covering the descriptor variants the ticket called out.
- `arb_descriptor()` (spec-shaped, yields `ModuleDescriptor`) plus
  `arb_kind()` (internal driver carrying port menus); `arb_plan()` /
  `arb_graph()` (1–10 nodes, edges filtered to `connect()`-safe
  proposals); `arb_edit()` / `arb_history()` (≤ 20 edits per history).
- Minimal `registry()` covers all five descriptor variants.
- Smoke properties: `graph_make_decisions_ok` and `history_build_patch_ok`.
- `ChangeParam` / `ChangeStructural` are present as `Edit` variants but
  apply as no-ops — `ModuleGraph` has no mutate-in-place API yet; 0982 /
  0983 can upgrade once it lands. Shrinker behaviour is unaffected.
- Inner-loop decision documented in the module doc: lives under `tests/`
  so `just inner -p patches-planner` picks it up; the default
  `inner_crates` set does not include `patches-planner`, so bare
  `just inner` does not run it.
- Runtime: ~0.13 s for both smoke properties on default case count.

## Summary

Stand up the property-based testing infrastructure that E161 builds on:
proptest as a dev dependency, a dedicated integration-test target so PBT
runtime does not slow `cargo test --lib`, generators for the inputs the
planner consumes (module descriptors, `ModuleGraph`s, replan edit histories),
and a minimal test registry that mirrors the descriptor pool so
`PatchBuilder::build_patch` succeeds on every generated graph. No asserted
properties yet — this is pure scaffolding so 0982 / 0983 can build on it.

## Acceptance criteria

- [ ] `proptest` added as a `[dev-dependencies]` entry on `patches-planner`.
- [ ] New integration-test target (e.g. `patches-planner/tests/properties.rs`,
      or a `properties/` directory) so the PBT runtime is isolated from
      `cargo test --lib` feedback.
- [ ] Generator `arb_descriptor()` returning `impl Strategy<Value =
      ModuleDescriptor>`, picking from a small fixed pool: single-mono-in /
      single-mono-out, multi-output Console-shaped (out / send_a / send_b),
      poly, stereo, sink-with-no-output. Reuses the 0976 minimal test registry
      shapes where they fit.
- [ ] Generator `arb_graph()` producing a `ModuleGraph` with 1–10 nodes and
      0–N edges, respecting `connect()`'s kind rules so every generated graph
      is plan-buildable (no `CableKindMismatch`).
- [ ] Generator `arb_edit()` over the variants `AddNode | RemoveNode | AddEdge
      | RemoveEdge | ChangeParam | ChangeStructural`.
- [ ] Generator `arb_history()` producing `Vec<arb_edit>` of bounded length
      (≤ 20 edits per history), applied left-to-right to a seed graph.
- [ ] A minimal test registry registers every descriptor variant the
      generators emit, so `PatchBuilder::build_patch` succeeds on every
      generated graph.
- [ ] A smoke property: "for every generated graph, `make_decisions` returns
      `Ok`" — no functional assertion, only confirms the generators stay
      within the well-formed domain.
- [ ] `just inner -p patches-planner` either includes the new target or
      explicitly excludes it (settle in implementation); document the choice.

## Notes

Part of epic **E161**, phase P0. The generators are load-bearing: the value of
0982 and 0983 depends on `arb_graph()` reaching the shapes that exercise the
slice-position / fusion / cycle-stability invariants.

Bias generators toward **small** graphs (≤ 10 nodes). Shrinker quality matters
more than coverage breadth — small counter-examples are debuggable, large ones
are not. The history generator caps at 20 edits for the same reason.

Do not reach for proptest-derive or `Arbitrary`-trait derivations: the structs
involved (`ModuleGraph`, `ParameterMap`) are not naturally `Arbitrary`, and
bespoke `Strategy` builders make the generator boundary explicit. The
multi-output Console shape is the highest-value descriptor variant — it is the
0974 regression's natural habitat.
