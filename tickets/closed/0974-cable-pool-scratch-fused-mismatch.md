---
id: "0974"
title: Investigate cable_pool scratch-slot / fused=false invariant violation
priority: medium
created: 2026-05-29
---

## Summary

Rendering several example synth patches headlessly through the default planner
trips the ADR 0072 phase-5 cable-pool invariant: a cable allocated a **scratch**
slot (planner decided *all* its consumers are fused, so it can live in a
same-tick scratch region) is later **read with `fused=false`**. The guard is the
`debug_assert!` in `CablePool::read_raw`
([patches-core/src/cable_pool.rs:99](../../patches-core/src/cable_pool.rs#L99)):

```text
scratch slot read with fused=false (cable_idx=NN);
scratch implies all consumers fused (ADR 0072 phase 5)
```

This means the planner's scratch-slot allocation and the per-consumer
`InputPort.fused` flags disagree for these graphs. It is a planner/builder
consistency bug, surfaced (not caused) during E159 / ticket 0973 auditioning;
the phase-representation migration touches no fusion logic. In release builds the
`debug_assert!` compiles out, so the read silently takes the scratch slot — which
may or may not hold the value the consumer expected, i.e. a potential **silent
stale/wrong-data read** in release (per the CLAUDE.md design note on fused-subgraph
ordering).

## Reproduction

Headless render via `patches_integration_tests::build_engine` + `run_n_stereo`
(the same helpers `alloc_trap.rs` uses). Observed panicking patches and their
offending `cable_idx`:

- `examples/pentatonic_sah.patches` — cable_idx 60
- `examples/poly_synth_layered.patches` — cable_idx 87
- `examples/synths/flute.patches` — cable_idx 47
- `examples/synths/solo_flute.patches` — cable_idx 49

Other Lfo/Op example patches rendered finite and bounded without tripping the
guard, so the issue is specific to some graph topology these four share. The
engine catches the panic at the tick boundary and halts cleanly (ADR 0051), so
existing debug tests that only check "no allocation" (e.g. `alloc_trap.rs`, which
already renders `pentatonic_sah.patches`) still pass — the violation is currently
invisible to the suite.

## Acceptance criteria

- [x] Minimal reproduction: a small `.patches` (or hand-built `ModuleGraph`) that
      trips the `read_raw` `debug_assert!`, captured as a regression test.
- [x] Root cause identified: why the planner assigns a scratch slot to a cable
      that has at least one `fused=false` consumer. Determine whether the bug is
      in (a) the planner's "all consumers fused" scratch-eligibility analysis, or
      (b) the builder setting `InputPort.fused` inconsistently for that consumer,
      or (c) a cross-SCC edge being mis-classified.
- [x] Fix so the invariant holds for all `examples/` patches; the four listed
      above render without tripping the guard.
- [x] Add a debug-build integration test that renders the affected example
      patches and asserts the engine does **not** halt (catches a re-regression
      that `alloc_trap` would silently swallow).
- [x] Confirm no silent wrong-data read existed in release for these patches
      (compare a known-correct reference render, or argue from the corrected slot
      assignment).
- [x] `just push` green; `just smoke` green (integration tests touched).

## Resolution

Root cause was **(a)** — a *key-space mismatch* between the planner's
scratch-eligibility analysis and buffer allocation:

- `classify_producer_ports` (`patches-planner/src/state/mod.rs`) keyed its
  per-producer-port cycle/scratch map by the edge's `out_idx`, which is
  `PortDescriptor::index` — the *user-visible* port number, scoped **per
  port-name group**. A 1-channel `Console`'s `out`, `send_a`, `send_b` all
  report `index == 0`.
- `allocate_buffers` and `build_input_buffer_map` key by the output port's
  **slice position** in `desc.outputs` (0, 1, 2 …).

For a multi-output module whose later port (e.g. `Console.send_b`) feeds a
feedback loop, the non-fused consumer's classification was recorded under
`(node, 0)` — colliding with the first output's key — while allocation queried
`(node, slice_pos)`, missed, and defaulted the port to a **scratch** slot. The
consumer's `InputPort.fused` flag (set correctly per-edge from `cable_fused`)
stayed `false`, so it read a scratch slot with `fused=false`. Confirmed against
`pentatonic_sah` (cable 60, reader `StereoDelay`, producer `Console.send_b`) and
`flute` (cable 47, reader `PolySvf`).

**Fix:** added `resolve_output_port_positions`, which maps each edge's producer
output to its slice position, and keyed `classify_producer_ports` by that —
aligning all three passes. Single-output modules are unaffected (slice position
== declared index == 0), so non-buggy patches are bit-identical; no audio
goldens reference the four affected patches.

**Release-mode safety (no silent wrong-data read):** under the bug the consumer
read the scratch single slot *dedicated to that exact producer port* — never
foreign or uninitialised data, since that slot is written by its producer every
tick and only zeroed on replan. The sole release effect was the feedback delay
on that one edge being correct or one sample early, depending on producer vs
consumer alphabetical order within the SCC (e.g. in `pentatonic_sah` consumer
`del` sorts before producer `mix`, so the scratch slot still held last-tick's
value — correct by accident). The fix restores deterministic cycle-pair
semantics regardless of intra-SCC ordering.

**Tests added / coverage hardened** (follow-up "tests should have caught this"
review):

- **Plan-build invariant** `validate_scratch_fused_consistency`
  (`patches-planner/src/state/mod.rs`): the build-time mirror of the
  `CablePool::read_raw` debug assert. Asserts every producer port in a scratch
  slot has only fused consumers. Runs once per plan build on the control thread,
  *outside* the per-tick `catch_unwind` (ADR 0051) that previously swallowed the
  runtime guard — so any future divergence fails the build directly, in every
  test that builds a plan. Hard `assert!` (matches `validate_fused_invariant`),
  off the audio hot path. Covered by two unit tests (panic + accept).
- `patches-planner` unit test
  `classify_producer_ports_later_output_slot_in_feedback_loop_is_cycle`: a
  hand-built two-module feedback loop where the producer's slice-1 output
  (declared `index 0`) must be classified cycle and allocated a cycle-region
  slot. Fails before the fix (port absent from the map → scratch).
- **Factored** the `(name, index) → slice position` port lookup — the
  duplicated, drift-prone logic at the root of the bug — into
  `ModuleDescriptor::output_position`/`input_position` (`patches-core`), and
  routed `resolve_output_port_positions` and `build_input_buffer_map` through
  it so there is one tested source of truth. Unit-tested at the lowest level
  (`port_position_distinguishes_slice_position_from_declared_index`) with a
  `Console`-like layout — the exact distinction the bug got wrong, asserted with
  no planner/engine/DSL in the loop.
- `patches-integration-tests/tests/render_no_halt.rs`: a **gating** sweep over
  frozen known-good fixtures (`tests/fixtures/`, independent of the WIP
  `examples/` tree) including `multi_output_feedback.patches` (Console `send_b`
  feedback through a delay), plus an **advisory** `#[ignore]` sweep over the live
  `examples/` tree. Replaces the original hand-picked `scratch_fused_invariant.rs`
  (only 7 of 36 examples were ticked, 4 asserting no-halt).
- `alloc_trap` `sweep()` hardened to also assert `halt_info().is_none()` — it
  ticked patches but checked only allocation, so a halted engine (which returns
  silence and may not allocate) passed silently.

## Notes

- Surfaced in E159 ticket 0973 audition; the four migrated modules
  (Lfo/PolyLfo/Op/PolyOp) are *not* the cause — the guard fires on cable indices
  set by the planner/builder regardless of a module's internal phase
  representation.
- Relevant design contract is in `CLAUDE.md` ("Parallelism-ready execution" /
  ADR 0072 fusion): within an acyclic fused subgraph, modules are emitted in topo
  order and consumers read producers' current-tick output via `fused = true`.
  Calling fused-subgraph modules out of topo order, or mis-flagging a consumer as
  unfused on a scratch cable, causes silent reads of stale data — exactly what
  this guard protects against.
- Start at `read_raw` ([cable_pool.rs](../../patches-core/src/cable_pool.rs)) and
  walk back to where scratch slots are allocated and where `InputPort.fused` is
  set in the builder/planner.
