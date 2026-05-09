---
id: E141
title: Cycle-free subgraph fusion
status: open
created: 2026-05-09
adr: 0072
---

## Summary

Restore audio-model integrity within cycle-free regions of the module
graph by removing the per-cable 1-sample delay where it is not needed
to break a feedback loop. Today every cable carries the delay
uniformly, which causes flams across mismatched-length parallel
chains, phase drift between sibling paths, and cumulative chain-length
latency leaking into audible timing. ADR 0072 specifies the
mechanism: SCC partitioning of the module graph, topo-sort within
each acyclic component, and per-cable fused/cyclic flagging so that
fused cables read producer output in the same tick instead of from
the previous tick's slot.

The rollout is phased so each step is independently shippable and
independently testable. Phase 1 lands the planner machinery with no
engine impact (audio output bit-identical to today). Phase 2 enables
fused reads, delivering the user-visible audio-integrity benefit.
Phases 3 and 4 are pure perf/footprint work, cancellable if
benchmarks do not motivate them.

Cycles continue to be allowed without warning, and feedback edges
continue to introduce a 1-sample delay. The 1-sample delay therefore
becomes a property of feedback structure rather than a uniform engine
characteristic.

## Tickets

- 0848 — Phase 1: SCC + condensation topo-sort + per-cable
  fused/cyclic annotation in the planner; engine inert (no read-side
  branch yet).
- 0849 — Phase 2: `InputPort.fused` flag + `CablePool::read_*` branch;
  fused reads pull from this-tick write slot. Audio-integrity tests.
- 0850 — Phase 3: two-region cable pool (scratch + cycle); FFI ABI
  decision; benchmark harness establishes before/after baseline.
- 0851 — Phase 4: scratch-region compaction + cache alignment +
  affinity-friendly partitioning.

## Sequencing

Strictly serial: 0848 → 0849 → 0850 → 0851. Each ticket's ship gate is
prerequisite for the next.

0848 lands without observable behaviour change; the planner emits
flags and topo order but the engine ignores them. Audio output
bit-identical to today.

0849 turns on the fused-read branch. Audio output changes for any
patch with a cycle-free chain longer than one module. Feedback
patches unchanged. After this phase the user-visible value is
delivered.

0850 reorganises the cable pool but preserves audio output bit-for-bit.
This phase is performance/footprint work and may be cancelled if
benchmarks do not motivate it. The FFI ABI decision (bump version vs
trampoline-emulate the old `[CableValue; 2]` shape) lives here.

0851 further compacts the scratch region for cache locality and
anticipates affinity partitioning. Cancellable independently of 0850.

## Out of scope

- **Module-merging fusion.** Collapsing cycle-free subgraphs into a
  single Module unit (with intermediates kept in registers) is a
  separate idea, parked under "module-merging fusion" in the
  speculative notes. ADR 0072 is independent and does not depend on
  or block module-merging fusion.
- **Parallel execution.** ADR 0072 partially constrains the existing
  trivial-parallelism story (modules within a fused subgraph must
  run in topo order) but parallel execution is a non-goal at this
  point. The fusion partitioning incidentally pre-computes the
  parallelism boundary should it ever be revisited.
- **User-visible feedback annotation.** No DSL syntax for marking
  cables as fused or delayed; the planner derives it from graph
  structure. Cables on a feedback arc are delayed; everything else
  is fused.
- **Sample-accurate routing across feedback boundaries.** A back
  edge always introduces a 1-sample delay; this ADR does not
  attempt to remove that.
