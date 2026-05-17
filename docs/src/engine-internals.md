# Engine internals

This chapter describes how Patches works under the hood: the compilation pipeline, the cable pool, the audio thread, plan handoff, and polyphony. It is aimed at contributors and anyone curious about the real-time architecture.

## Compilation pipeline

A `.patches` file goes through four stages before audio runs:

```
.patches source
     │
     ▼
  Parser (patches-dsl)          PEG grammar → AST with source spans
     │
     ▼
  Expander (patches-dsl)        Inline templates, substitute parameters → FlatPatch
     │
     ▼
  Interpreter (patches-interpreter)  Validate against module registry → ModuleGraph
     │
     ▼
  Planner (patches-planner)     Graph → ExecutionPlan, reusing surviving modules
     │
     ▼
  Audio thread                  Tick loop: execute plan, one sample at a time
```

**Parser.** The PEG grammar in `patches-dsl/src/grammar.pest` defines the syntax. The parser (Pest-based) produces an AST with source location spans preserved for error reporting. The output is a `File` struct containing template definitions and a patch block.

**Expander.** Template instantiation happens here. Each `module v : voice(...)` is expanded into a copy of the template's modules and connections, with names mangled to avoid collisions (e.g. `v/osc`, `v/env`). Parameter references are substituted. Cable scales are composed by multiplication at template boundaries. The output is a `FlatPatch` — a flat list of `FlatModule` and `FlatConnection` structs with no template nesting.

**Interpreter.** The `FlatPatch` is validated against the module registry. Module type names are resolved to descriptors. Parameters are checked against their declared types and ranges. Cable kinds (mono/poly) are verified to match between connected ports. The output is a `ModuleGraph` — a directed graph of typed nodes and edges.

**Planner.** The planner converts the `ModuleGraph` into an `ExecutionPlan` — a flat, ordered list of module slots and buffer assignments that the audio thread can execute without any graph traversal. The planner carries state between builds: it matches modules in the new graph against the previous plan by name and type, reusing existing instances (with their state intact) where possible. New modules are freshly instantiated; removed modules are listed as tombstones for cleanup.

## The cable pool

Inter-module signals live in a single pool split into two regions:

- **Scratch region** — `[0, SCRATCH_CAPACITY)`. One `CableValue` per slot;
  consumers read producers' *current-tick* output. Used by every cable
  inside a fused (acyclic) subgraph.
- **Cycle region** — `[SCRATCH_CAPACITY, SCRATCH_CAPACITY + CYCLE_CAPACITY)`.
  `[CableValue; 2]` ping-pong pair per slot; consumers read the *previous*
  tick's value. Used by cables that carry feedback across an SCC boundary.

A single virtual `cable_idx` selects the region: indices below
`SCRATCH_CAPACITY` route to scratch, the rest to cycle. Capacities are
fixed at compile time (`SCRATCH_CAPACITY = 2048`, `CYCLE_CAPACITY = 128`);
the planner reports `BufferPoolExhausted` if a patch overflows either
region.

`CableValue` is a fixed 16-lane `f32` slot:

```rust
#[repr(transparent)]
pub struct CableValue(pub [f32; 16]);
```

The cable's declared kind — `Mono`, `Stereo`, or `Poly` — is static and
tells reader and writer which prefix of the array carries data: lane 0
for `Mono`, lanes 0–1 for `Stereo`, all 16 lanes for `Poly`. There is no
runtime tag. Constructors `CableValue::mono(v)`, `stereo(l, r)`, and
`poly([..; 16])` zero the unused lanes; accessors `as_mono`, `as_stereo`,
`as_poly` read only the relevant prefix. Holding all slots at 16 lanes
lets a slot used for `Mono` in one plan be repurposed as `Poly` in the
next without reallocating the pool.

**Subgraph fusion.** Across SCCs the planner emits modules in
topological order and routes cross-SCC cables through the scratch region,
so consumers read producers' current-tick output. This removes the
per-cable one-sample lag that would otherwise accumulate audibly across
long signal chains, without changing the patch author's mental model.
Cycle-bearing cables (those that close a feedback loop) live in the cycle
region and retain the 1-sample delay, which makes them well-defined
regardless of execution order. Calling fused modules out of topological
order causes silent reads of stale data — this matters when extending the
planner, not when authoring patches.

### Reserved slots

The first `RESERVED_SLOTS = 32` scratch indices are reserved for
infrastructure and never allocated to user cables. They are split into a
**backplane** at the bottom and **sinks** at the top of the reserved
range:

| Index | Name                    | Kind   | Purpose                                                       |
| ----- | ----------------------- | ------ | ------------------------------------------------------------- |
| 0     | `AUDIO_OUT_L`           | mono   | Left audio output — `AudioOut` writes, callback reads         |
| 1     | `AUDIO_OUT_R`           | mono   | Right audio output                                            |
| 2     | `AUDIO_IN_L`            | mono   | Left audio input (`AudioIn`)                                  |
| 3     | `AUDIO_IN_R`            | mono   | Right audio input                                             |
| 4     | `GLOBAL_TRANSPORT`      | poly   | Transport frame — sample count + tempo + position  |
| 5     | `GLOBAL_DRIFT`          | mono   | Slowly varying value in `[-1, 1]` for correlated pitch drift  |
| 6     | `GLOBAL_MIDI`           | poly   | Packed MIDI frame — up to 5 events per sample      |
| 7–10  | `TAP_BASE..+4`          | poly   | `Tap` module storage — 4 slots × 16 lanes = 64 tap channels   |
| 11–14 | `HOST_CONTROL_BASE..+4` | poly   | Host-control values — 4 slots × 16 lanes = 64 controls        |
| 15–27 | —                       | —      | Spare backplane capacity (no live mapping)                    |
| 28    | `MONO_READ_SINK`        | mono   | Disconnected mono inputs read zero from here                  |
| 29    | `POLY_READ_SINK`        | poly   | Disconnected poly inputs read zero from here                  |
| 30    | `MONO_WRITE_SINK`       | mono   | Unconnected mono outputs write harmlessly here                |
| 31    | `POLY_WRITE_SINK`       | poly   | Unconnected poly outputs write harmlessly here                |

The sinks sit at the top of the reserved range so the FFI plugin loader can shift plugin-visible scratch past the backplane while
still resolving plugin-relative `[0, SINK_SLOTS)` to the sink slots. The
backplane → sink layout is asserted at compile time.

Disconnected ports point at the sink slots. This means `process` never
needs to branch on connectivity — it can always call
`pool.read_mono(&self.port)` safely, getting zero for unconnected
inputs. Modules that want to *skip work* for disconnected outputs can
check `output.is_connected()`.

### Buffer stability

The cable pool persists across re-plans. Unchanged cables keep their buffer index, so CV signals on cables that were not rewired continue without interruption. Recycled and newly allocated slots are listed in the execution plan's `to_zero` vector; the audio thread zeroes them before the first tick of the new plan.

Pool capacities are fixed compile-time constants
(`SCRATCH_CAPACITY = 2048`, `CYCLE_CAPACITY = 128`). The planner allocates
dyn-scratch indices above `RESERVED_SLOTS` via a high-water mark; cycle
indices are allocated in the cycle region in build order.

## Audio thread constraints

The audio callback runs under hard real-time constraints. Violating them causes audible glitches.

**No allocations.** All buffers and module state are pre-allocated when the plan is built. The callback never calls `Box::new`, `Vec::push` with growth, or any other heap-allocating operation.

**No blocking.** No mutexes, no file or network I/O, no syscalls that may sleep.

**No deallocation.** Dropping a `Box<dyn Module>` can run arbitrary destructor code. All deallocation is routed to a background thread (see below).

**Unwinding panic policy.** The CLAP host and any crate loaded as an FFI plugin must build with `panic = "unwind"`. `PatchProcessor::tick` catches module panics at the tick boundary and halts the engine cleanly — that only works with table-based unwinding. `panic = "abort"` in the workspace or plugin profiles defeats the catch and takes the host process down with the panic.

Communication with the control thread uses an **rtrb** lock-free single-producer / single-consumer ring buffer. The callback polls it at the start of each processing block.

## The execution plan

`ExecutionPlan` is the audio thread's working document. It contains:

- **slots** — one `ModuleSlot` per active module, listing its pool index and the buffer indices for each of its input and output ports (with scale factors for scaled inputs).
- **active_indices** — the execution order. The audio thread iterates this list and calls `process` on each module.
- **to_zero / to_zero_poly** — buffer indices to zero before the first tick.
- **new_modules** — freshly instantiated modules to install in the pool.
- **tombstones** — pool indices of removed modules, to be sent to the cleanup thread.
- **parameter_updates** — changed parameter maps for surviving modules.
- **port_updates** — new port assignments for modules whose wiring changed.

The plan is built on the control thread and sent to the audio thread as a single value via the ring buffer. Once received, the audio thread applies it in one block: zero buffers, tombstone old modules, install new ones, apply updates, then resume ticking.

## Plan handoff

The sequence when a patch file is saved:

1. The control thread builds a new `ExecutionPlan` via the planner.
2. `PatchEngine` sends the plan through the rtrb ring buffer.
3. The audio callback checks for a new plan at the top of each processing block.
4. If a plan is waiting: the current plan is replaced via `mem::replace`. The old plan is pushed to the cleanup ring buffer as a `CleanupAction::DropPlan`.
5. The callback applies the new plan's updates (zeroing, tombstoning, installation, parameter and port updates) and transitions to the new execution state.
6. Processing resumes with the new plan.

There is a brief window between when the planner snapshots the old module state and when the audio thread installs the new plan. Module state (e.g. oscillator phase) advances during this window. This is an intentional trade-off — the alternative would require stopping the audio thread, which would cause a gap.

## Module lifecycle

Every module instance has an immutable `InstanceId` — a monotonically increasing `u64` assigned at construction. The planner uses the combination of the module's DSL name and type name to match modules across reloads.

The module pool is a `Vec<Option<Box<dyn Module>>>` owned by the audio thread. Surviving modules keep their pool slot. New modules are inserted at free slots. Tombstoned modules are extracted from their slot and pushed to the cleanup thread.

The `set_ports` and `update_validated_parameters` methods are called on the audio thread when a surviving module's wiring or parameters change. Both are required to be non-allocating and infallible.

## Off-thread deallocation

Dropping a module or an old execution plan runs destructors that may allocate or block. A dedicated thread named `patches-cleanup` handles this. It owns the consumer end of a lock-free ring buffer and drains `CleanupAction` values:

```rust
enum CleanupAction {
    DropModule(Box<dyn Module>),
    DropPlan(Box<ExecutionPlan>),
    DropParamState(Box<ParamState>),
    DropParamFrame(Box<ParamFrame>),
    DropMonitorMeta(Box<MonitorMeta>),
    DropHostControlPlanMeta(Box<HostControlPlanMeta>),
}
```

The audio thread pushes to this buffer; the cleanup thread drops the values on its own time. If the buffer is full (which should not happen in normal operation), the audio thread falls back to dropping inline with a warning.

## Cable kinds

The system has three cable kinds:

- **Mono** — one `f32` per tick. The default.
- **Poly** — `[f32; 16]` per tick (one value per voice). Voice count is
  fixed at engine initialisation and shared by all poly cables.
- **Stereo** — `[f32; 2]` per tick, carried as `(L, R)`. Storage
  reuses the poly slot (lanes 0–1); pool layout is unchanged.

Layouts (`MonoLayout::Audio`/`Trigger`, `PolyLayout::Audio`/`Trigger`/
`Transport`/`Midi`) refine within `Mono` and `Poly`. Stereo carries no
layout — it is exclusively audio/CV.

### Connection rules

| From          | To       | Result                                          |
| ------------- | -------- | ----------------------------------------------- |
| Mono          | Mono     | direct (layouts must match)                     |
| Poly          | Poly     | direct (layouts must match)                     |
| Stereo        | Stereo   | direct                                          |
| Mono Audio    | Stereo   | **broadcast**: `(s, s)`           |
| Stereo        | Mono     | rejected (`CableKindMismatch`)                  |
| Poly ↔ Stereo | —        | rejected                                        |
| Mono ↔ Poly   | —        | rejected (use `MonoToPoly` / `PolyToMono`)      |

Mono→stereo broadcast is consumer-side: the planner sets
`StereoInput::broadcast_from_mono`, the cable still points at the mono
producer's slot, and `pool.read_stereo()` returns `(s, s)` from the
underlying `CableValue::Mono`. No synthetic node, no extra audio-thread
work. Stereo→mono needs an explicit `StereoSplitter`.

### Polyphony

Poly modules (`PolyOsc`, `PolyAdsr`, `PolyVca`, etc.) process each voice
independently within a single `process` call. Their ports read and
write `[f32; 16]` arrays via `pool.read_poly` and `pool.write_poly`.

Poly cable slots are zeroed with `Poly([0.0; 16])` rather than
`Mono(0.0)` to prevent type mismatches during the first tick after a
hot-reload.

## Periodic updates

Some modules need to recompute internal coefficients when their CV inputs change — for example, a filter whose cutoff is controlled by a cable. Recomputing on every sample would be expensive, so the `Module` trait exposes a periodic hook:

```rust
pub trait Module {
    // ...
    fn wants_periodic(&self) -> bool { false }
    fn periodic_update(&mut self, _pool: &CablePool<'_>) {}
}
```

Modules whose `wants_periodic()` returns `true` override `periodic_update`
to read their CV inputs and update filter coefficients, frequency
targets, or other derived state. The planner queries `wants_periodic()`
once at plan-build time and collects matching slot indices; every
`periodic_update_interval` samples (typically a few dozen), the audio
thread dispatches `periodic_update` through the normal
`&mut dyn Module` path. The per-sample `process` call then uses these
precomputed values, keeping the hot path fast.

This replaces an earlier `PeriodicUpdate` trait and its
`as_periodic()` lookup, which were removed along with their
raw-pointer caching footgun.

## Observability

Each runtime's FFI data plane carries a small set of counters that a
non-real-time observer can sample at any time. They are updated with
`Relaxed` atomics on the hot paths — no allocation, no blocking, safe
from the audio thread — and exposed via `RuntimeArcTables::snapshot()`
(control side) or `RuntimeAudioHandles::snapshot()` (audio side).

`RuntimeCountersSnapshot` contains:

- `param_frames_dispatched` — cumulative count of `ParamFrame`
  deliveries. The dispatcher increments this via
  `RuntimeAudioHandles::note_param_frame_dispatched()`.

The earlier `FloatBuffer` / `ArcTable<[f32]>` counters were retired with
the pipeline itself; the snapshot surface is intentionally
small now.

The attach API that routes counters to a UI or logging sink is still
deferred; until then, tests and soak runs read them directly through
the snapshot accessors.
