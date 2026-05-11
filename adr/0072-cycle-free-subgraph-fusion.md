# ADR 0072 — Cycle-free subgraph fusion

## Status

Proposed (2026-05-09)

## Context

Every cable in the engine carries a one-sample delay. The pool is
double-buffered: each cable owns a `[CableValue; 2]` slot pair, with a
write index `wi` that flips on every tick. Reads return slot `1 - wi`
(last tick), writes go to slot `wi` (this tick). ADR 0015 introduced
this delay deliberately, citing two consequences:

1. **Order-independence.** Module execution order does not affect
   correctness. The tick scheduler can pick any order — alphabetical,
   declaration order, anything — and the patch produces the same
   output. This is what `compute_order` in
   [patches-planner/src/state/mod.rs:268-272](../patches-planner/src/state/mod.rs#L268-L272)
   relies on (it just sorts node ids alphabetically).
2. **Lock-free parallel writes.** Because no module reads what another
   module writes *this tick*, the write phase has no inter-module
   dependencies. Threads can write into disjoint cable slots without
   synchronisation.

The cost is **audio model integrity**, not performance. The per-cable
delay is uniform but accumulates along the cable, so two signals that
the patch source treats as simultaneous can arrive at a downstream
mixer or sidechain on different ticks depending on path length.

Concretely:

- **Flams across parallel chains.** A trigger fanned to
  `kick_chain → mixer.in_a` and `snare_chain → mixer.in_b`, where
  `kick_chain` has three modules and `snare_chain` has five, lands at
  the mixer two samples apart. For a percussive transient that is a
  flam — an audible doubling of the attack rather than a single hit.
  The patch source describes a unison; the engine produces a smear.
- **Phase drift between siblings.** A sine doubled into two filter
  paths of different lengths emerges with a few samples of relative
  offset. Summed back together, the offset is a comb filter the
  patch did not ask for. Phase coherence between sibling paths is a
  property the patch source implies but the engine does not preserve.
- **Cumulative group delay along chains.** Chain length acts as a
  hidden, structural latency. Patch authors cannot reason about it
  without knowing the engine's internals; refactoring a chain
  (inserting or bypassing a module) silently changes the timing of
  everything downstream relative to everything not downstream.

For most patches, most cables sit inside cycle-free subgraphs. A
filter-bank fan-out, a serial FX chain, an envelope-into-VCA — none
of these contain feedback. Only intentional feedback structures
(Karplus-Strong, comb resonators, FM operators with self-modulation,
state-variable filter resonance loops) actually require the delay
to break a cycle.

The 1-sample delay is therefore over-applied. Within a cycle-free
subgraph, modules can be executed in topological order and read each
other's *current-tick* output directly, restoring the timing semantics
the patch source implies: simultaneous fan-outs arrive simultaneously,
sibling paths stay phase-coherent, chain length stops leaking into
audible timing.

## Decision

The planner will partition the module graph into strongly connected
components (SCCs). Within each acyclic SCC (i.e. each SCC of size 1
that is not self-looping, plus any SCC the topo-walk can flatten),
modules are emitted in topological order, and the cables connecting
them are flagged as **fused**. Fused cables are read from the
current-tick write slot (`wi`) instead of the previous-tick read
slot (`1 - wi`). Cables that cross SCCs, or sit on a feedback edge
within a non-trivial SCC, retain the existing 1-sample delay.

The decision is per-cable, not per-module. A module may participate
in fused reads on some inputs and delayed reads on others — for
instance, a VCA whose audio input comes from a serial chain (fused)
and whose CV comes from an LFO that closes a feedback loop elsewhere
(delayed).

### Algorithm

At plan build time, after the connection graph is finalised but
before `active_indices` is populated:

1. **Build a directed dependency graph.** For each cable `A.out → B.in`,
   add an edge `A → B`.
2. **Compute SCCs** via Tarjan's algorithm.
3. **Condense** the graph: each SCC becomes a single node; inter-SCC
   edges are preserved.
4. **Topo-sort** the condensation. Within each SCC, modules can be
   ordered arbitrarily for non-trivial SCCs; for trivial SCCs (single
   module, no self-loop), order is determined by the condensation's
   topo-sort.
5. **Emit `active_indices`** in the order produced by step 4.
6. **Flag fused cables.** A cable `A.out → B.in` is fused iff `A` and
   `B` are in different SCCs *and* `A` precedes `B` in the topo-sort.
   (The "different SCCs" condition holds for all cycle-free patches;
   the topo-sort condition is automatic from step 4.) Cables within a
   non-trivial SCC are never fused — they are the cycle-breaking
   edges.

### Engine change

`InputPort` gains a `fused: bool` flag. `CablePool::read_*` methods
branch on it:

```rust
let slot = if input.fused { self.wi } else { 1 - self.wi };
```

Modules are unchanged: they still call `pool.read_mono(&self.input)`
without knowing or caring which buffer they are reading.

The planner sets `input.fused` on each `InputPort` at plan adoption,
based on the cable annotation from step 6.

### What stays the same

- Module trait, `process(&mut self, pool)` signature, port machinery.
- Cable buffer layout (`[CableValue; 2]` per cable, ping-pong indexing).
- `CableKind` (Mono, Poly, Stereo) and the broadcast rules.
- Hot-reload, observation, FFI, the entire DSL surface.
- Cycle handling: cycles continue to be allowed without warning, and
  feedback edges continue to introduce a one-sample delay.

## Consequences

### Audio model integrity restored within acyclic regions

Simultaneous fan-outs arrive simultaneously at downstream mixers and
sidechains regardless of the chain length on either branch. Sibling
paths remain phase-coherent under summing. Chain length stops being
a hidden source of timing offset. A patch's audible behaviour now
matches the topology its source describes.

Concretely: the flam-on-fan-out problem disappears for any trigger
fed through cycle-free chains; comb-filter artefacts from
mismatched-length parallel filter paths disappear; envelope-driven
dynamics settle on the sample the gate fires rather than N ticks
later.

### Order now matters within fused subgraphs

The CLAUDE.md desideratum "modules can run in any order" is partially
relaxed. Modules in a fused subgraph **must** be called in
topological order; calling them out of order causes silent reads of
stale data (the slot would still be holding last tick's value, since
the producer hasn't run yet this tick).

The planner enforces the order by construction. Validation at plan
build time asserts that for every fused cable `A.out → B.in`,
`A`'s position in `active_indices` is less than `B`'s.

### Behavioural change to existing patches

A patch's sample-by-sample output may differ from its pre-fusion
output. The change is one of timing, not of intent: a chain that
was effectively `y[n] = f(x[n - k])` for some chain length `k` is
now `y[n] = f(x[n])`. For most patches this is closer to what the
author intended.

Feedback patches are unaffected because the cycle-breaking edge
remains delayed.

We do not provide a "preserve old behaviour" toggle. The 1-sample
chain delay was an implementation artefact, not a semantic feature
of the DSL. Patches that depended on it for some chosen offset
should use an explicit `Delay(samples=N)` module.

### Parallel execution: complicated, but a non-goal

The original 1-sample-delay invariant made parallelism trivial: any
two modules can write into their cable slots concurrently because no
read in the same tick observes those writes. Fused cables break that
invariant — a module reading a fused cable must wait for the
producer to have completed.

In a parallel scheduler, this would force fused subgraphs onto a
single thread, or introduce per-cable synchronisation. SCCs would
become natural parallel units (independent SCCs run on independent
threads), but modules within an acyclic chain would serialise.

We accept this. **Parallel execution is not a near-term goal.** The
synchronisation overhead — atomic stores, cache-line bouncing,
thread wakeup — is likely to outweigh the gain for most patches at
typical buffer sizes (32–256 samples). When and if parallelism
becomes a real target, the fusion graph already provides the right
abstraction: SCCs partition the graph into units that *must* run
sequentially internally and *can* run in parallel externally. Fused
cables become intra-thread (cheap, no synchronisation); inter-SCC
cables become inter-thread (rare, bounded).

The fusion decision is therefore not at odds with future
parallelism — it pre-computes the parallelism boundary. It is at
odds with the *trivial* parallelism of "any module can run in any
order on any thread", but that scheme was never going to be
performant anyway.

### Buffer layout — interim (phase 2) and target (phase 3)

**Interim**: cable buffers remain `[CableValue; 2]` for every cable.
The write phase still writes to slot `wi`. The slot flip at
end-of-tick is unchanged. A fused read reads the slot that was
written this tick instead of last tick — both slots are always
valid memory. This is the minimum-change form: only the read-side
branch changes.

A cable feeding both a fused consumer and a delayed consumer is
supported trivially in this form: the producer writes to slot `wi`
once; the fused consumer reads slot `wi`; the delayed consumer
reads slot `1 - wi`. No duplicate writes, no extra storage.

**Target**: a two-region pool exploits the fact that fused slots
carry no inter-tick state.

```text
[ scratch_0, ..., scratch_M | cycle_0_a, cycle_0_b, cycle_1_a, cycle_1_b, ... ]
  └─ fused: 1 slot/cable ──┘ └── cycle: 2 slots/cable, ping-pong on wi ──┘
```

- **Scratch region.** One `CableValue` per fused cable. Producer
  writes; consumer reads (same slot, same tick). Slot value at
  start of tick is irrelevant — it is always overwritten by the
  producer before any consumer reads it. No `wi` indexing required.
- **Cycle region.** Two `CableValue` slots per cycle cable, with
  the existing `wi` ping-pong semantics. Holds the 1-tick state
  the back edge depends on.

A producer port whose cable feeds *any* delayed consumer must live
in the cycle region (the delayed consumer needs last tick's value;
that requires double-slotting). A producer port whose consumers
are all fused lives in the scratch region. The decision is per
producer port, derived from the maximum requirement across its
consumers.

In typical patches the feedback arc set is small (1–5 cables),
so the cycle region is a fraction of the scratch region. Net
storage: roughly half of today's pool. Cache density on the hot
path (forward DAG traversal) improves correspondingly because
fused slots pack contiguously.

Read dispatch becomes a tagged index:

```rust
match cable.slot {
    Slot::Scratch(idx) => pool[idx],
    Slot::Cycle(pair)  => pool[pair + (1 - wi)],
}
```

(or two physically separate buffers — `scratch_pool: Vec<CableValue>`
and `cycle_pool: Vec<[CableValue; 2]>` — chosen at phase-3
implementation time based on benchmarks).

### Replanning and slot lifetime

Fused slots and cycle slots have different lifetimes across plan
boundaries.

**Fused slot — stateless.** The slot value at the start of any
tick is irrelevant: the producer overwrites it before any consumer
reads (topo order guarantees this). Therefore at replan, fused
slot indices can be reshuffled freely. Plan A puts cable X at
scratch index 7; plan B puts cable X at scratch index 42. The
audio thread observes no discontinuity — first tick after swap,
the producer writes to 42 and the consumer reads 42.

**Consequence: the planner has full freedom over the scratch
region's layout on every replan.** Optimise for cache locality,
thread-affinity partitioning, descriptor proximity, anything.
No state-preservation constraint.

**Cycle slot — 1-tick state.** Slot `1 - wi` holds last tick's
written value, which the back edge consumer reads this tick.
Discarding it on replan resets the feedback path: delay tail
collapses, resonant filter loses ring, FM operator self-mod
restarts from zero. Audible click.

The planner therefore identifies cycle cables that persist across
replans by `(producer_module_name, producer_output_port_name)`
and copies the old pool's `[CableValue; 2]` pair into the
new pool at adoption. This reuses the module-state migration
mechanism from ADR 0002.

Cycle cables that are *new* in the post-replan plan (because the
patch grew a new feedback path) initialise to default — one tick
of silence on that edge while it spins up, identical to a
cold-start of a feedback patch.

Cycle cables that *disappear* (the patch removed a feedback edge,
or the cable became fused because its consumer left the SCC) are
dropped: their values were either superseded by a fresh fused
slot or no longer read at all.

**Net property: scratch indices float; cycle indices are stable
in value, free in layout.** The planner can re-pack cycle pairs
freely too (the migration map handles old-index → new-index
copying), but the *values* must persist. Scratch slots have
neither constraint: nothing to copy, free to relocate.

### Cycle detection becomes a permanent planner phase

Today the planner has no cycle detection. After this change, SCC
computation runs on every plan build. Tarjan's algorithm is `O(V + E)`
and the graphs are small (typically dozens of modules); the cost is
negligible.

The diagnostic surface improves as a side effect: the LSP can
optionally surface "this cable is in a feedback loop" as an inlay
hint, since the planner now knows.

### Hot-reload behaviour

Plan rebuild already runs through the same builder pipeline, so
hot-reload picks up the new fusion partitioning automatically. A
patch edit that adds or removes a feedback edge will move cables
between fused and delayed states on the next reload — the engine
swaps to the new plan atomically as today.

## Phased rollout

Each phase is independently shippable and independently testable.
Earlier phases unlock value without committing to later ones.

### Phase 1 — Graph analysis only

Add SCC computation, condensation topo-sort, and per-cable
fused/cyclic flagging to the planner. No engine change. The
flags are emitted into the `ExecutionPlan` and the
`active_indices` ordering respects the topo-sort, but the engine
still reads from `1 - wi` for every cable.

This phase is pure analysis. It is testable in isolation:

- Tarjan's SCC against contrived graphs (single module, simple
  cycle, nested cycles, disjoint subgraphs).
- Topo-sort against ordering invariants (for every fused cable,
  producer index < consumer index in `active_indices`).
- Cable annotation against expected feedback-arc-set size for a
  battery of representative patches.
- Diff `active_indices` against today's alphabetical ordering on
  the test corpus and confirm audible output unchanged (engine
  ignores the flag, so audio behaviour must be identical).

**Ship gate**: all existing patches produce bit-identical output
to today. New planner machinery exercised but inert.

### Phase 2 — Fused reads with unchanged pool layout

Add the `fused: bool` flag to `InputPort`, branch
`CablePool::read_*` on it. Cable buffer layout still
`[CableValue; 2]` per cable; producer writes both slots'
worth of memory available; only the read side changes.

This is the smallest engine change that delivers the audio-model
benefit. Validation focuses on:

- Audio integrity tests: flam-on-fan-out goes away; sibling-path
  phase coherence holds; envelope-driven dynamics settle in
  one tick.
- Regression suite for feedback patches (Karplus-Strong, comb
  resonator, FM self-mod, resonant SVF) — output must match
  today's bit-for-bit on the cycle-bearing edges.
- Property test: order-of-execution within a fused subgraph must
  match topo-sort. Detected violations are planner bugs, not
  engine bugs.

**Ship gate**: audio-integrity benefits demonstrated on the
test corpus; feedback patches unchanged; no regressions found.
After this phase, the user-visible value is delivered. Phase 3
and 4 are pure performance/footprint work.

### Phase 3 — Two-region pool layout

Migrate the cable pool to the scratch + cycle structure
described above. Fused cables get one slot; cycle cables get
the existing pair.

Considerations specific to this phase:

- **FFI impact.** Modules loaded via FFI receive cable buffer
  pointers across the ABI boundary. The current ABI assumes
  `[CableValue; 2]` per cable. Migrating fused cables to single
  slots changes the pointer-arithmetic contract. Either
  (a) bump the FFI ABI version and update the host-side trampoline
  to dispatch on slot kind, or (b) keep the FFI surface
  emulating `[CableValue; 2]` (write the same value to both
  positions of a virtual pair) at the trampoline, paying a
  per-call cost for ABI stability. Decide based on FFI plugin
  count at the time of phase 3.
- **Benchmarks before and after.** Before-phase-3 baseline
  captures the phase-2 performance (full double-buffered pool
  with fused reads). After-phase-3 measures the two-region pool.
  Expected wins: lower memory footprint, denser cache lines on
  forward DAG traversal. Expected risk: tagged-index dispatch
  (`match cable.slot`) may cost more than today's branchless
  index arithmetic on hot paths. Benchmark on representative
  patches (drums, FX-heavy, mod-matrix-heavy) before committing.
- **Two physical buffers vs one tagged buffer.** Two separate
  `Vec`s (`scratch_pool`, `cycle_pool`) avoid tagged dispatch
  but cost a second base pointer in the cable pool struct.
  Single tagged pool keeps locality but adds a branch on every
  read. Decide via benchmark; the ADR does not pre-commit.
- **Replanning copy logic.** Implement the
  `(producer_module_name, producer_output_port_name)` →
  `[CableValue; 2]` migration map for cycle cables at plan
  adoption. Reuse module-state migration plumbing.

**Ship gate**: equivalent or better runtime perf, lower memory
footprint, FFI plugins still work. Bit-identical audio output to
phase 2.

### Phase 4 — Scratch allocation optimisation

Compact scratch indices into the lowest region of the pool with
producer-before-consumer ordering matching `active_indices`.
Cycle pairs occupy reserved upper indices, possibly grouped by
SCC.

Optimisations enabled:

- **Cache-line alignment.** Pack scratch slots so that a
  cache-line-sized window of the pool corresponds to a topo
  slice of the graph. Forward traversal hits each line once,
  warm.
- **Affinity partitioning.** Anticipating future parallelism,
  partition scratch indices by SCC-cluster so that any
  thread-assignment scheme places contiguous slots on the same
  thread.
- **Allocator simplicity.** Once layout is fixed by topo order,
  scratch allocation is a single forward sweep — no free list,
  no fragmentation, no reuse. Cycle region is a small reserved
  area allocated separately.

**Ship gate**: incremental perf wins on cache-bound workloads
(mixer trees, large mod matrices, dense fan-out patches).
Cancellable: phases 1–3 stand on their own if phase 4
benchmarks do not motivate it.

### Phase 5 — Scratch-first layout invert (ticket 0860, E143)

Phase 3 placed cycle at `[0, CYCLE_CAPACITY)` and scratch at
`[CYCLE_CAPACITY, …)`, with the backplane embedded inside scratch
at `[CYCLE_CAPACITY, CYCLE_CAPACITY + RESERVED_SLOTS)`. That
layout left `- CYCLE_CAPACITY` arithmetic scattered across the
engine, harness, and tests every time a backplane slot was read
or written, and pinned disconnected-input defaults at
`fused: false` despite their constant-zero read semantics being
inherently same-tick.

Phase 5 inverts the index space:

| range                                                   | region   | content                                |
|---------------------------------------------------------|----------|----------------------------------------|
| `[0, SINK_SLOTS)`                                       | scratch  | sinks (mono/poly read/write)           |
| `[SINK_SLOTS, RESERVED_SLOTS)`                          | scratch  | backplane (audio I/O, transport, …)    |
| `[RESERVED_SLOTS, SCRATCH_CAPACITY)`                    | scratch  | dyn scratch (planner-allocated)        |
| `[SCRATCH_CAPACITY, SCRATCH_CAPACITY + CYCLE_CAPACITY)` | cycle    | dyn cycle producers                    |

Three load-bearing decisions:

1. **Scratch is low, cycle is high.** Inverts the phase-3
   framing. Backplane is the most-trafficked reserved region
   and is naturally single-slot; making it the bottom of the
   index space gives small const literals
   (`AUDIO_OUT_L = 4`, `GLOBAL_TRANSPORT = 8`, …) and zero
   arithmetic at engine write sites.
2. **Sinks move to scratch.** Read sinks are never written;
   write sinks are never read. Neither needs ping-pong
   storage. Falling out of (1): sinks at scratch `[0, 4)`,
   backplane at scratch `[4, 32)`. `init_cycle_pool` no
   longer special-cases sink slots.
3. **`fused: true` is the default for disconnected inputs.**
   Disconnected ports route to a sink in scratch (constant
   zero, same-tick). The only transition out of `fused: true`
   is being wired to a delayed-consumer cycle producer, which
   the planner sets explicitly.

`CablePool` dispatch becomes a single cutoff:
`cable_idx < SCRATCH_CAPACITY → scratch[cable_idx]`, else
`cycle[cable_idx - SCRATCH_CAPACITY]`. The planner's logical
cycle hwm lives in `[0, CYCLE_CAPACITY)`; absolute `cable_idx`
emitted into modules is `SCRATCH_CAPACITY + logical`.

**ABI bump**: FFI plugin ABI version moves from v10 to v11. No
external plugin clients today; the in-tree consumers
(patches-vintage, test plugins, host loader) recompile against
the new constants. Plugin SDKs that bake any of the backplane
literals need rebuild.

**Audio churn**: same as phase 3 — patches that read transport,
drift, or MIDI advance one sample sooner than under the
pre-fusion plan because backplane reads are now inherently
same-tick. Goldens are auditioned, not silently regenerated.

### Phase 6 — Backplane low, sinks high (ticket 0869, E145)

Phase 5 placed sinks at scratch `[0, SINK_SLOTS)` and the backplane
at `[SINK_SLOTS, RESERVED_SLOTS)`. That worked while the FFI plugin
loader exposed the full scratch view, but it made every backplane
addition or shift an externally-visible ABI change — already paid
once in v11.

Phase 6 swaps the two regions within the reserved range:

| range                                                  | region   | content                              |
|--------------------------------------------------------|----------|--------------------------------------|
| `[0, RESERVED_SLOTS - SINK_SLOTS)`                     | scratch  | backplane (audio I/O, transport, …)  |
| `[RESERVED_SLOTS - SINK_SLOTS, RESERVED_SLOTS)`        | scratch  | sinks (mono/poly read/write)         |
| `[RESERVED_SLOTS, SCRATCH_CAPACITY)`                   | scratch  | dyn scratch (planner-allocated)      |
| `[SCRATCH_CAPACITY, SCRATCH_CAPACITY + CYCLE_CAPACITY)`| cycle    | dyn cycle producers                  |

The structural change is internal: backplane/sink symbols keep their
names, only their numeric values shift. The motivating value lands
in ticket 0870, which shifts the plugin-visible scratch base past
the backplane so plugin-relative `[0, SINK_SLOTS)` resolves to the
sink slots unchanged. After 0870, future backplane reorgs no longer
force ABI bumps.

The spare capacity between the live backplane top
(`HOST_CONTROL_BASE + HOST_CONTROL_SLOTS`) and the first sink slot
(`RESERVED_SLOTS - SINK_SLOTS`) is room for new backplane slots
without bumping `RESERVED_SLOTS`. A `const _: () = assert!(…)` in
[patches-core/src/cables/mod.rs](../patches-core/src/cables/mod.rs)
guards the invariant at compile time.

**ABI bump**: FFI plugin ABI version moves from v11 to v12. Same
client set as phase 5 — in-tree only, recompile against the new
constants.

**Audio churn**: none. The reorg is purely a relabelling of reserved
slots; cable values, planner ordering, and module behaviour are
unchanged.

## Alternatives considered

**Always read from current write buffer (no fusion analysis).** Reject
on first principles: would deadlock-read on cycles. The 1-sample
delay exists precisely to break cycles; we cannot remove it
unconditionally.

**User-annotated fusion (`-->` for fused, `->` for delayed).** Pushes
the SCC analysis onto the user. Most users do not think about cable
delay; making them annotate it is a footgun and clutters the surface
syntax. Rejected.

**Buffer reordering at the planner level (move-to-front).** Re-pack
the buffer so producers are written contiguously before consumers,
then read directly. Equivalent to fusion in effect but harder to
implement: requires reasoning about cable identity across the pool
layout, and complicates double-buffering. Fusion via a per-port flag
is strictly simpler.

**Detect fusion opportunities only at build time when a profile
flag is set.** Complicates the build matrix and asks the user to
opt in to a correctness improvement. The default should be the
better behaviour; cycles already opt out by their structure.

**Run two scheduling regimes in parallel — fused for low-latency
audio, delayed for control-rate.** Rejected as premature. The
current proposal already handles per-cable selection; if a future
need arises (e.g. control-rate paths benefiting from delay for
stability), it would slot into the same per-cable-flag mechanism.

## References

- ADR 0015 — Cable double-buffering and 1-sample delay
- ADR 0067 — Blast-radius cuts within the monorepo
- CLAUDE.md "Design desiderata" — parallelism-ready execution
- [patches-core/src/cable_pool.rs:65-102](../patches-core/src/cable_pool.rs#L65-L102)
- [patches-planner/src/state/mod.rs:268-272](../patches-planner/src/state/mod.rs#L268-L272)
- [patches-planner/src/builder/mod.rs:187-272](../patches-planner/src/builder/mod.rs#L187-L272)
- [patches-engine/src/execution_state.rs:259-287](../patches-engine/src/execution_state.rs#L259-L287)
