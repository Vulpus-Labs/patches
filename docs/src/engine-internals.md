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
  Planner (patches-engine)      Graph → ExecutionPlan, reusing surviving modules
     │
     ▼
  Audio thread                  Tick loop: execute plan, one sample at a time
```

**Parser.** The PEG grammar in `patches-dsl/src/grammar.pest` defines the syntax. The parser (Pest-based) produces an AST with source location spans preserved for error reporting. The output is a `File` struct containing template definitions and a patch block.

**Expander.** Template instantiation happens here. Each `module v : voice(...)` is expanded into a copy of the template's modules and connections, with names mangled to avoid collisions (e.g. `v/osc`, `v/env`). Parameter references are substituted. Cable scales are composed by multiplication at template boundaries. The output is a `FlatPatch` — a flat list of `FlatModule` and `FlatConnection` structs with no template nesting.

**Interpreter.** The `FlatPatch` is validated against the module registry. Module type names are resolved to descriptors. Parameters are checked against their declared types and ranges. Cable kinds (mono/poly) are verified to match between connected ports. The output is a `ModuleGraph` — a directed graph of typed nodes and edges.

**Planner.** The planner converts the `ModuleGraph` into an `ExecutionPlan` — a flat, ordered list of module slots and buffer assignments that the audio thread can execute without any graph traversal. The planner carries state between builds: it matches modules in the new graph against the previous plan by name and type, reusing existing instances (with their state intact) where possible. New modules are freshly instantiated; removed modules are listed as tombstones for cleanup.

## The cable pool

All inter-module signals live in a single contiguous allocation: a `Vec<[CableValue; 2]>`. Each cable occupies one slot in this vector. The two-element inner array is a ping-pong pair.

```rust
pub enum CableValue {
    Mono(f32),
    Poly([f32; 16]),
}
```

`CableValue` is `Copy`. On each tick, the write index `wi` alternates between 0 and 1. Modules write to `pool[cable_idx][wi]` and read from `pool[cable_idx][1 - wi]`. This gives every cable a one-sample delay: a module always reads the value that was written on the *previous* tick. The consequence is that execution order does not matter — modules can be scheduled in any order and the results are identical. Feedback connections are also well-defined: they carry the previous tick's value.

### Reserved slots

The first 16 slots are reserved for infrastructure and never allocated to user cables:

| Index | Name | Purpose |
|-------|------|---------|
| 0 | `MONO_READ_SINK` | Disconnected mono inputs read zero from here |
| 1 | `POLY_READ_SINK` | Disconnected poly inputs read zero from here |
| 2 | `MONO_WRITE_SINK` | Unconnected mono outputs write harmlessly here |
| 3 | `POLY_WRITE_SINK` | Unconnected poly outputs write harmlessly here |
| 4 | `AUDIO_OUT_L` | Left audio output — AudioOut writes, callback reads |
| 5 | `AUDIO_OUT_R` | Right audio output |
| 6 | `AUDIO_IN_L` | Left audio input (reserved) |
| 7 | `AUDIO_IN_R` | Right audio input (reserved) |
| 8 | `GLOBAL_CLOCK` | Absolute sample counter, written by callback each tick |
| 9 | `GLOBAL_DRIFT` | Slowly varying value in [-1, 1] for globally correlated pitch drift |
| 10–15 | — | Reserved for future use |

Disconnected ports point at the sink slots. This means `process` never needs to branch on connectivity — it can always call `pool.read_mono(&self.port)` safely, getting zero for unconnected inputs. Modules that want to *skip work* for disconnected outputs can check the `connected` field on their output port.

### Buffer stability

The cable pool persists across re-plans. Unchanged cables keep their buffer index, so CV signals on cables that were not rewired continue without interruption. Recycled and newly allocated slots are listed in the execution plan's `to_zero` vector; the audio thread zeroes them before the first tick of the new plan.

The pool has a fixed capacity (default 4096 slots). A freelist tracks available indices for allocation.

## Audio thread constraints

The audio callback runs under hard real-time constraints. Violating them causes audible glitches.

**No allocations.** All buffers and module state are pre-allocated when the plan is built. The callback never calls `Box::new`, `Vec::push` with growth, or any other heap-allocating operation.

**No blocking.** No mutexes, no file or network I/O, no syscalls that may sleep.

**No deallocation.** Dropping a `Box<dyn Module>` can run arbitrary destructor code. All deallocation is routed to a background thread (see below).

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
    DropPlan(ExecutionPlan),
}
```

The audio thread pushes to this buffer; the cleanup thread drops the values on its own time. If the buffer is full (which should not happen in normal operation), the audio thread falls back to dropping inline with a warning.

## Polyphony

The system supports two cable kinds: mono (a single `f32` per tick) and poly (an `[f32; 16]` array — one value per voice per tick). The voice count is fixed at engine initialisation (default 16) and shared by all poly cables in a patch.

Poly modules (`PolyOsc`, `PolyAdsr`, `PolyVca`, etc.) process each voice independently within a single `process` call. Their ports read and write `[f32; 16]` arrays via `pool.read_poly` and `pool.write_poly`.

Connecting a mono output to a poly input (or vice versa) is a validation error caught by the interpreter. `MonoToPoly` broadcasts a mono value to all 16 voices. `PolyToMono` sums all voices down to a single mono signal.

Poly cable slots are zeroed with `Poly([0.0; 16])` rather than `Mono(0.0)` to prevent type mismatches during the first tick after a hot-reload.

## Periodic updates

Some modules need to recompute internal coefficients when their CV inputs change — for example, a filter whose cutoff is controlled by a cable. Recomputing coefficients on every sample would be expensive, so the engine supports a `PeriodicUpdate` trait:

```rust
pub trait PeriodicUpdate {
    fn periodic_update(&mut self, pool: &CablePool<'_>);
}
```

Modules that implement this trait (and return `Some(self)` from `as_periodic()`) have their `periodic_update` method called every N samples (a configurable interval, typically a few dozen samples). This is where they read their CV inputs and update filter coefficients, frequency targets, or other derived state. The processing in `process` then uses these precomputed values, keeping the per-sample path fast.
