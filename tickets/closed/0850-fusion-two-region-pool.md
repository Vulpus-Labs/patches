---
id: "0850"
title: Fusion phase 3 — two-region cable pool (scratch + cycle), FFI ABI decision, benchmark harness
priority: low
created: 2026-05-09
epic: E141
adr: 0072
depends-on: "0849"
---

## Summary

Migrate the cable pool from uniform `[CableValue; 2]` per cable to
a two-region layout: a **scratch** region with a single
`CableValue` per fused cable, and a **cycle** region with the
existing pair per cable on a feedback arc. This halves the
common-case storage and densifies cache lines along the forward
DAG traversal. The layout split is the long-term home for the
fused/cyclic distinction; the read branch from phase 2 is
replaced by a tagged-index dispatch.

Phase 3 is performance/footprint work. Audio output must remain
bit-identical to phase 2.

## Acceptance criteria

- [ ] Cable slot identity moves from a single index into a tagged
      enum:
      ```rust
      enum CableSlot {
          Scratch(usize),       // index into scratch region
          Cycle(usize),         // index of [_; 2] pair in cycle region
      }
      ```
      (or two separate `Vec`s — see below.)
- [ ] Producer-side allocation: a producer port whose cable feeds
      *any* delayed consumer is assigned a `Cycle` slot. A producer
      port whose consumers are all fused is assigned a `Scratch`
      slot. The decision is per producer port, derived from the
      max requirement across consumers, computed at plan build
      time.
- [ ] `CablePool::read_*` dispatches on slot kind. `Scratch(idx)`
      reads `pool[idx]` directly. `Cycle(pair)` reads
      `pool[pair + (1 - wi)]`.
- [ ] `CablePool::write_*` likewise: `Scratch(idx)` writes
      `pool[idx]`; `Cycle(pair)` writes `pool[pair + wi]`.
- [ ] Replanning: cycle-slot values persist across plan adoption.
      Build a `BTreeMap<(ModuleName, OutputPortName),
      [CableValue; 2]>` from the old plan's cycle slots; on
      adoption, for each cycle cable in the new plan with a key
      match, copy the old pair into the new pool. New cycle cables
      initialise to `CableValue::default()`. Reuse the
      module-state migration plumbing from ADR 0002.
- [ ] Scratch slots reset to `CableValue::default()` on plan
      adoption (no value carried — they are stateless across ticks).
- [ ] FFI ABI decision documented and implemented:
      - **Option A**: bump FFI ABI version; host trampoline
        dispatches on slot kind; FFI plugins recompiled.
      - **Option B**: trampoline emulates `[CableValue; 2]` by
        writing scratch values to both positions of a virtual pair;
        FFI plugins unchanged at the cost of a per-call write
        amplification on the FFI boundary.
      - Choose based on FFI plugin count at the time of phase 3.
        If the count is small (today: `gain`, `conv-reverb`),
        prefer Option A.
- [ ] Benchmark harness compares phase-2 baseline vs phase-3 on
      representative patches:
      - drum kit (mixer-tree-heavy)
      - mod-matrix-heavy patch (mostly fused, tiny FAS)
      - resonant feedback patch (cycle-heavy)
      - large fan-out (single producer to many consumers)
      Captured metrics: average tick duration, p99 tick duration,
      L1 / L2 cache miss rate (perf counters), pool memory
      footprint.
- [ ] Decide: single tagged pool vs two physical buffers
      (`scratch_pool: Vec<CableValue>`, `cycle_pool:
      Vec<[CableValue; 2]>`) based on benchmark results. ADR 0072
      does not pre-commit; this ticket commits.
- [ ] All audio-integrity and feedback-regression tests from 0849
      still pass with bit-identical output.
- [ ] `just commit` passes; `just push` passes (FFI plugin scan
      included).

## Notes

The producer-side max-requirement rule is the only subtlety: a
single producer port may feed both fused and delayed consumers.
That port's slot must be a `Cycle` pair (the delayed consumer
needs last-tick's value). In typical patches this is rare; most
producer ports are exclusively fan-out to fused consumers and
land in the scratch region.

Cycle slot value migration is the same pattern as module-state
migration. Identity by `(producer_module_name,
producer_output_port_name)`. If the producer module is renamed
or removed, drop. If the producer's output port newly becomes a
cycle source (because the patch grew a feedback edge that
includes it), initialise to default — equivalent to a one-tick
silence on that edge while the feedback path spins up. This is
identical to cold-starting a feedback patch and has been
acceptable behaviour to date.

FFI Option A is preferred if feasible. The current FFI ABI
already changed shape during E113 (panic-unwind plugin contract);
adding a slot-kind discriminator is a compatible evolution. The
trampoline becomes:

```text
fn host_read(plugin_handle, port_id) -> CableValue {
    match plan.slot_for(port_id) {
        Scratch(idx) => pool[idx],
        Cycle(pair)  => pool[pair + (1 - wi)],
    }
}
```

The plugin-side surface is unchanged: it still receives
`CableValue` from `host_read` and passes `CableValue` to
`host_write`. The dispatch happens on the host side of the ABI.

If benchmarks show no win, the ticket may close as "not
motivated" with a note documenting the measured cost. Phase 4 is
cancellable independently.
