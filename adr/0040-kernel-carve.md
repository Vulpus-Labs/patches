# ADR 0040 — Carve a stable kernel: registry, planner, cpal, host

**Date:** 2026-04-17
**Status:** proposed

## Context

The workspace currently fuses several conceptually distinct phases into
a few large crates. In particular:

- `patches-engine` owns the builder, planner, execution state, kernel,
  processor, and module pool — and also the cpal stream, audio
  callback, MIDI, and WAV I/O. Any embedding that wants the execution
  machinery drags cpal along with it.
- The module registry lives in `patches-core/src/registries/`. Every
  consumer that needs to register or look up a module builder depends
  on the full `patches-core`, and the eventual FFI/WASM plugin loading
  surface will need to grow there without cluttering the core.
- `patches-player` and `patches-clap` duplicate ~250 lines of
  composition: registry init, DSL pipeline driving, planner
  construction, plan-channel wiring, processor spawn.

At the same time, the crates that *are* already well-separated — the
pure DSP kernels in `patches-dsp`, the DSL pipeline in `patches-dsl`,
and the descriptor-bind + graph build in `patches-interpreter` — show
the value of the existing boundaries: they are independently testable,
their surfaces are stable, and LSP/CLAP/player can each compose only
what they need.

Two near-future scenarios push the same way:

- **Kernel externalization.** Once observability (trace capture, cable
  state inspection, module event broadcast — design exists, not yet
  built) and the FFI ABI are stable, we want to break the monorepo: LSP
  and SVG move out, drums externalize as a plugin bundle, and the ABI
  itself is published so third parties can build modules. A prerequisite
  is that `patches-core` + the execution machinery form a small,
  audit-able kernel that external consumers depend on, not the current
  hairball where touching the audio callback can ripple into DSL parsing.
- **Dynamic plugin loading in more hosts.** The LSP and CLAP plugin will
  both eventually load FFI/WASM module builders at runtime. They need a
  registry surface that is ergonomic for dynamic registration without
  dragging the whole execution machinery into their dependency trees.

## Decision

Carve four new crates out of the existing workspace. Keep the crates
that are already well-shaped.

### New crates

1. **`patches-registry`** — extracted wholesale from
   `patches-core/src/registries/` (`registry.rs`, `module_builder.rs`,
   `file_processor.rs`, `mod.rs`). Exposes: native module registration,
   external module path scanning, FFI/WASM plugin loading. Depends on
   `patches-core`. `patches-core` itself does not depend on it — the
   core graph/plan/module types are registry-agnostic.

2. **`patches-planner`** — extracted from `patches-engine/src/planner.rs`
   and `patches-engine/src/builder/mod.rs`. Owns `Planner`,
   `ExecutionPlan`, and `PlannerState` (the graph-diffing state
   currently at `patches-core/src/graphs/planner/`). `ModuleGraph`
   itself remains in `patches-core` — it is a foundational topology
   type, not planning state. `PlannerState` moves because it is
   ephemeral cross-build machinery that belongs with the planner that
   owns it, not with the configuration type it analyses.

3. **`patches-cpal`** — extracted from `patches-engine/src/callback.rs`,
   `patches-engine/src/input_capture.rs`, and the cpal-specific portions
   of `patches-engine/src/engine.rs`. Wraps cpal stream creation and the
   audio callback. Used by `patches-player`; future alternative audio
   backends (JACK, offline render, test harness) live alongside as peers.

4. **`patches-host`** — new crate bundling the composition shared by
   `patches-player` and `patches-clap`: registry init, DSL pipeline
   driving, planner construction, plan-channel wiring, processor spawn.
   Exposes traits for the divergent bits (`HostFileSource`,
   `HostAudioCallback`) so each binary plugs in its own file source,
   audio callback structure, transport/MIDI handling, and GUI.

### Crates that stay

- **`patches-dsl`** keeps the loader. `pipeline.rs` already exposes the
  right surface (source bytes → FlatPatch + diagnostics) and the crate
  is pure: only `pest` + `patches-core`. Splitting a thin wrapper crate
  would add a Cargo.toml for no dependency reduction — every Loader
  consumer already depends on `patches-dsl` for the AST types.
- **`patches-interpreter`** keeps its current shape: bind + build +
  errors. Both halves are "interpretation" (validate descriptor-level,
  then instantiate into a `ModuleGraph`), and the same callers (LSP,
  player, CLAP) need both. Renaming to `patches-binder` would narrow
  the perceived scope without improving cohesion. No registry or loader
  code lives inside patches-interpreter today, so there is nothing to
  split out from underneath it.
- **`patches-engine`** remains, but slimmed: kernel, executor, pool,
  execution state, processor. Backend-agnostic after the cpal
  extraction. Applies `ExecutionPlan` diffs emitted by `patches-planner`.

### Resulting consumer composition

| Consumer | Uses |
|---|---|
| `patches-player` | host + cpal + (transitively) registry, planner, engine, interpreter, dsl |
| `patches-clap` | host + (transitively) registry, planner, engine, interpreter, dsl |
| `patches-lsp` | interpreter + registry + dsl |
| `patches-svg` | (whatever it uses today — not in scope) |

LSP's exclusion of planner + engine is the payoff: it gets a bounded
graph through the full DSL → bind path without ever linking the audio
execution machinery or cpal.

## Rationale

**Prepare for monorepo breakup.** A stable kernel (`patches-core` +
`patches-engine` + `patches-registry` + `patches-planner` +
`patches-interpreter` + `patches-dsl`) becomes the published surface
that external plugins, external LSP/SVG crates, and future third-party
hosts depend on. The current shape — where execution machinery and
cpal are fused — forces every downstream consumer to depend on desktop
audio I/O or accept a subtle "use some of engine but not all" contract.

**Narrow focus, lower blast radius for changes.** With cpal in
`patches-engine`, a cpal version bump touches the same crate as the
audio callback logic and the builder — obscuring what actually
changed. After the carve, cpal bumps are confined to `patches-cpal`,
planner refactors are confined to `patches-planner`, and registry API
evolution is confined to `patches-registry`. Integration-test blast
radius shrinks correspondingly.

**Prepare for dynamic plugins in LSP and CLAP.** Both will grow the
ability to load FFI/WASM builders at runtime. A dedicated
`patches-registry` crate gives that surface a home that neither drags
the audio execution machinery along nor clutters `patches-core`.

**Clarify and decouple phases.** The pipeline is: *load* (DSL) → *bind*
(interpreter descriptor-bind) → *build* (interpreter graph construction)
→ *plan* (planner) → *execute* (engine). Each phase has a distinct
input type, output type, and failure mode. The current crate boundaries
mirror this only partially: planning and execution share a crate, and
the registry — required by bind and plan but not by core — lives in
core. The carve aligns crate boundaries with phase boundaries, which
makes the pipeline legible in `Cargo.toml` as well as in code.

## Consequences

**Positive**

- Kernel surface becomes bounded and publishable. External consumers
  (future out-of-tree LSP, SVG, plugin projects) know what to depend on.
- cpal dependency is contained; non-cpal embeddings (CLAP, offline
  render, tests) do not pull it in.
- Planner evolution (ADR 0012 graph-diffing, future multi-thread
  execution splitting) happens in its own crate rather than inside the
  engine grab-bag.
- Player and CLAP share a single composition layer; new consumers
  (future CLI variants, test harnesses, alternate hosts) get the same
  starting point instead of re-implementing the wiring.
- Each new crate is independently testable with a minimal dependency
  set.

**Negative**

- Five crates (four new + slimmed engine) increase the `Cargo.toml`
  count and compile-graph fan-out. Incremental compile times improve
  for most edits but clean builds cost slightly more.
- `patches-host`'s trait shape (`HostFileSource`, `HostAudioCallback`)
  is a design call that will bend under the first two consumers.
  Expect iteration after player and CLAP port over.
- The planner split coincides with ADR 0012 (planner v2 graph-diffing,
  status *proposed*). Moving `PlannerState` out of `patches-core`
  should be coordinated with that work so we do not carve twice.

**Neutral**

- No behaviour change. This is a crate-boundary refactor; every type
  and function retains its identity, just under a new path. Consumers
  update imports.
- No relationship to the observability blocker for external plugin
  distribution. This ADR is a prerequisite for that work but can land
  independently.

## Blast radius

### `patches-registry` extraction

- Move: `patches-core/src/registries/{registry,module_builder,file_processor,mod}.rs`.
- Update imports in: `patches-engine`, `patches-interpreter`, `patches-lsp`,
  `patches-clap`, `patches-player`, `patches-modules`, `patches-ffi`,
  `patches-ffi-common`, `patches-wasm`, `patches-integration-tests`,
  `patches-svg`.
- `patches-core` stops re-exporting registry types. Callers that used
  `patches_core::Registry` adjust to `patches_registry::Registry`.

### `patches-planner` extraction

- Move: `patches-engine/src/planner.rs`, `patches-engine/src/builder/`
  (the `ExecutionPlan` side; kernel-facing wiring stays in engine),
  `patches-core/src/graphs/planner/` (`PlannerState`, `graph_index`,
  decision classification).
- `ModuleGraph` stays at `patches-core/src/graphs/graph/`.
- Update imports in: `patches-engine` (now depends on planner),
  `patches-clap`, `patches-player`, `patches-integration-tests`.
- Coordinate with ADR 0012 if that work is active.

### `patches-cpal` extraction

- Move: `patches-engine/src/callback.rs`, `patches-engine/src/input_capture.rs`,
  cpal-specific portions of `patches-engine/src/engine.rs`.
- `patches-engine/src/engine.rs` splits: backend-agnostic setup (sample
  rate, env construction, processor spawn) stays; cpal stream creation
  and device negotiation move.
- `patches-player` depends on the new crate; `patches-clap` does not
  (it uses host-provided audio).

### `patches-host` extraction

- New crate. Pulls composition logic from `patches-player/src/main.rs`
  (load/bind/build + engine setup + hot-reload loop structure) and
  `patches-clap/src/plugin.rs` + `patches-clap/src/factory.rs` (the
  equivalent composition).
- Defines `HostFileSource`, `HostAudioCallback`, `HostPatchSource`
  traits for the divergent bits. Provides a `HostBuilder::new(registry,
  sample_rate) → (Planner, Processor, plan_channel)` and a patch-load
  helper.
- `patches-player/src/main.rs` and `patches-clap/src/plugin.rs` shrink
  to the integration layer plus their respective audio and event
  handling.

### Engine slim-down

- `patches-engine` loses: `planner.rs`, `callback.rs`, `input_capture.rs`,
  cpal portions of `engine.rs`, MIDI routing if it turns out to be
  cpal-specific.
- `patches-engine` keeps: `kernel.rs`, `execution_state.rs`, `pool.rs`,
  `processor.rs`, `decimator.rs`, `oversampling.rs`, backend-agnostic
  `engine.rs` shell.
- `wav_recorder.rs` moves to `patches-io` (already exists).
- MIDI location is a judgement call deferred to the corresponding
  ticket — stays in engine if cross-embedding, otherwise moves to host
  or a new `patches-midi` crate.

### Dependency-graph shape after

```
patches-dsp   patches-core ── patches-registry
     │             │                │
     └──────┬──────┘                │
            │                       │
      patches-dsl ── patches-interpreter
                          │           │
                          │           │
                    patches-planner ──┤
                          │           │
                     patches-engine ──┤
                          │           │
                 ┌────────┤           │
                 │        │           │
           patches-cpal   │           │
                 │        │           │
                 └────────┴── patches-host
                              │        │
                        ┌─────┘        └─────┐
                  patches-player      patches-clap
```

`patches-lsp` and `patches-svg` sit on
`patches-interpreter` + `patches-registry` + `patches-dsl` only; neither
touches planner, engine, cpal, or host.
