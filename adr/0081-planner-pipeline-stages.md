# ADR 0081 — Planner as a typed analysis / validation / transformation pipeline

- Status: accepted
- Date: 2026-05-29
- Supersedes: none
- Related: ADR 0072 (subgraph fusion / scratch–cycle cable regions), ADR 0051
  (panic = unwind, tick-boundary halt), ADR 0067 (tiered validation profiles)

## Context

`patches-planner` turns a `ModuleGraph` into an `ExecutionPlan` (+ a carried
`PlannerState` for stable replans). Today it is two phases behind
`Planner::build`:

1. **Decision phase** — `make_decisions` (state/mod.rs). Pure:
   `(graph, prev_state, capacity) → PlanDecisions`. Internally chains
   sub-stages: `GraphIndex::build` → `compute_order_with_fusion`
   (`tarjan_scc`) → `validate_fused_invariant` → `resolve_output_port_positions`
   → `classify_producer_ports` → `allocate_buffers` →
   `validate_scratch_fused_consistency` → `classify_nodes`.
2. **Action phase** — a ~360-line block inside
   `PatchBuilder::build_patch_with_meta`. Impure: instantiates modules via
   `registry.create`, mints `InstanceId::next()`, packs param frames, builds
   `InputPort`/`OutputPort` objects, assembles the `ExecutionPlan` and the new
   `PlannerState`.

This works, but three properties make it hard to evolve and hard to test at the
level where bugs actually live:

- **Stage boundaries are implicit.** Sub-stages exchange bare tuples
  (`(order, cable_fused, fas_size)`) and loose `HashMap`s, then dump everything
  into one grab-bag `PlanDecisions`. There is no `IR_{n-1} → IR_n` typing, so a
  stage cannot be expressed — or tested — as "transform the previous stage's
  output."
- **Errors are signalled by panic.** The two internal invariants
  (`validate_fused_invariant`, `validate_scratch_fused_consistency`) `assert!` /
  `panic!`. Deviant conditions can only be tested with `#[should_panic]`, which
  is coarse (cannot assert *which* violation) and conflates "internal bug" with
  "testable outcome."
- **The action phase is an impure monolith.** Pure transformation (build port
  objects, partition inputs, pack frames, assemble slots) is interleaved with
  side effects (`registry.create`, `set_ports`, id minting). The pure logic
  cannot be exercised without wiring up a real registry and real modules, so its
  core has near-zero direct unit tests.

Ticket 0974 is the cautionary tale. A producer output port's *slice position*
was derived **independently in three places** (`classify_producer_ports`,
`allocate_buffers`, `build_input_buffer_map`); one used the wrong key, the three
diverged, and a feedback cable was mis-allocated. The runtime guard that should
have caught it was a `debug_assert` swallowed by the tick-boundary
`catch_unwind` (ADR 0051). The root enabler was **re-derivation of the same fact
at multiple sites** — i.e. parameter churn between stages.

Note on scope: *user-facing* validation (unknown module, port / cable-kind
mismatch) lives upstream in `patches-interpreter`. The planner receives an
already-valid `ModuleGraph`. The planner's validation is therefore
**internal-invariant** checking, not re-validation of user input.

## Decision

Restructure plan-building as an explicit, typed pipeline of stages, each a
function `fn(IR_prev) -> Result<IR_next, PlanError>`, in three kinds:

- **Analysis** — derive facts from prior IR (connectivity, SCC/topo order,
  fused classification, port-slot classification, install/update decisions).
- **Validation** — check an invariant over prior IR and return `Result`,
  transforming nothing.
- **Transformation** — produce a new IR (buffer/module allocation; the final
  `ExecutionPlan`).

### 1. Result-based error signalling

Internal-invariant violations return a structured `PlanError`, not a panic. Add
variants (e.g. `FusedOrderViolation`, `ScratchFusedConflict`) carrying the
offending node/port so a test can assert the *specific* deviant condition with a
plain `Result`, no `#[should_panic]`. `PlanError` continues to map into
`BuildError` for callers. A `debug_assert` backstop may remain where a state is
believed strictly unreachable, but it must never be the *only* signal.

### 2. Typed IR bundles, frozen and composed — no parameter churn

Each stage emits a named bundle. A bundle is **finalised once and passed forward
frozen**; later stages must **not** re-map its fields into fresh structs or
re-derive its facts. Later IRs **compose** earlier frozen bundles by embedding
the ones they still need.

The rule has one exception: when a bundle (or some of its fields) is no longer
needed, **discard it / narrow the interface** rather than carry dead data into
later representations. (Example: the raw `SccPartition` can be dropped once
`order` + `cable_fused` are derived, if nothing downstream reads it.)

This is the direct structural fix for the 0974 class: a fact such as a producer
port's slice position is computed **once**, frozen into its bundle, and read by
every later stage from that single source — never re-derived. (See also the
`ModuleDescriptor::output_position` consolidation already landed for 0974, which
this generalises.)

Indicative IRs (names/shapes to be settled in implementation):

| IR | Kind | Carries | Notes |
|----|------|---------|-------|
| `GraphIndex` | analysis | edges, connectivity | exists |
| `Topology` | analysis | `order`, `cable_fused`, `fas_size` | from SCC; partition discarded if unused |
| `PortClassification` | analysis | `out_port_pos`, `producer_port_cycle` | computed once, read by allocate + validate + action |
| `BufferLayout` | transform | `BufferAllocation`, `ModuleAllocDiff`, resolved input buffers | embeds prior frozen bundles, does not re-flatten them |
| `NodeDecisions` | analysis | per-node install/update vs `prev_state` | exists as `Vec<(NodeId, NodeDecision)>` |
| `PlanDraft` | transform | everything needed to build the plan **without** instantiating modules | pure; the action shell's input |

### 3. Pure transformation, isolated effects

Split the action phase into pure transforms (`PlanDraft` → port objects, slot
layout, param-frame plans — testable with descriptors only) behind a thin impure
shell whose sole job is the unavoidable effects: `registry.create`,
`module.set_ports`, and id minting. Inject the `InstanceId` source (an allocator
passed in) so the pure path is deterministic under test instead of depending on
the global `InstanceId::next()` counter.

### 4. Every stage extensively unit-tested

Each stage is independently constructible (build `IR_prev` by hand, assert
`IR_next`) and must carry direct unit tests for happy path, edge cases, and —
now that they return `Result` — deviant/error conditions. The decision-phase
stages already follow this (SCC, order/fusion, `classify_nodes`, graph-index
have good direct coverage); the gaps to close are `allocate_buffers` cable
logic, the action-phase transforms, the orchestrators, and replan/state
threading.

## Alternatives considered

- **Leave the two-phase structure, just add tests.** Rejected as insufficient:
  the action phase cannot be unit-tested at the transformation level without a
  registry, and the implicit tuple boundaries keep inviting the re-derivation
  that caused 0974. Tests alone do not remove the structural hazard.
- **Accumulating-context struct threaded through every stage** (one growing
  `PlanContext` that all stages read/write). Rejected as the default: it clutters
  late stages with early data and blurs which stage owns which field. Composition
  of frozen bundles with explicit narrowing keeps ownership and lifetimes clear;
  the no-churn rule plus the discard exception is exactly this trade-off.
- **Keep panics for internal invariants** (they "can't happen"). Rejected: 0974
  showed an "impossible" divergence shipping, and a panic is swallowed by the
  tick-boundary `catch_unwind`. `Result` makes the deviant path a first-class,
  testable outcome on the control thread.
- **Big-bang rewrite.** Rejected: the planner is load-bearing and feeds the
  realtime engine. The change is staged and behaviour-preserving, gated by the
  golden + integration suite and the plan-build invariant.

## Consequences

- A clear, typed pipeline that reads as analysis → validation → transformation;
  each stage testable in isolation, deviant conditions assertable via `Result`.
- The "compute a fact once, freeze, pass forward" rule removes the re-derivation
  hazard behind 0974 by construction.
- The pure/impure split lets the bulk of plan construction be unit-tested
  without a registry; the impure shell shrinks to instantiation only.
- Migration is incremental (epic E160): lock-in tests first, then IR types, then
  the action-phase split, then Result-based validation, then coverage fill.
  Goldens for migrated examples/fixtures must stay bit-identical — this is a
  structural refactor, not a behaviour change.
- One coexistence cost during migration: stages partially typed and partially
  tuple-based until E160 completes; the order in E160 minimises that window.

## Open questions

1. **IR ownership vs lifetimes.** Several IRs borrow the graph (`GraphIndex<'a>`).
   Whether `PlanDraft` should own copies (so the action shell is `'static`-clean)
   or keep borrowing is an implementation call in E160.
2. **Granularity of `PlanError` variants.** How finely to subdivide internal
   invariants vs a single `Internal { context }` — settle as the validation
   stages are written; favour distinct variants where a test wants to assert a
   specific condition.
3. **Injecting `InstanceId`.** Whether to thread an allocator trait or a simple
   `&mut u64` seed; decided in the action-phase split ticket.
