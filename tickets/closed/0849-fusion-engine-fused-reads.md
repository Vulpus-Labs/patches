---
id: "0849"
title: Fusion phase 2 — fused reads in CablePool, audio integrity tests
priority: medium
created: 2026-05-09
epic: E141
adr: 0072
depends-on: "0848"
---

## Summary

Wire the per-cable `fused` flag emitted by the planner (ticket 0848)
into the engine's read path. Add `fused: bool` to `InputPort`;
`CablePool::read_*` methods branch on it: `fused` reads from slot
`wi` (this tick's writes), non-fused reads continue from `1 - wi`
(last tick's writes). Cable buffer layout is unchanged — every
cable still owns `[CableValue; 2]`. Only the read side changes.

After this ticket, audio output for any patch with a cycle-free
chain longer than one module changes: timing offsets that came from
chain-length latency disappear. Feedback patches are unchanged
because the back edge remains delayed.

## Acceptance criteria

- [ ] `InputPort` gains a `fused: bool` field. Set by the planner
      at plan adoption from the cable annotation produced in 0848.
- [ ] `CablePool::read_mono`, `read_stereo`, `read_poly` (and any
      other read variants) branch on `input.fused`, selecting slot
      `wi` or `1 - wi` accordingly. Module code unchanged.
- [ ] Cable buffer layout unchanged: still `[CableValue; 2]` per
      cable. Producer writes slot `wi` once; both consumer kinds
      read from the same buffer pair.
- [ ] Audio-integrity tests added under
      `patches-integration-tests/`:
      - Flam-on-fan-out: trigger fanned through chains of differing
        length lands at a downstream mixer in the same sample.
      - Sibling phase coherence: a sine doubled into two filter
        paths of different module-counts emerges with zero relative
        offset.
      - Envelope-driven dynamics: a gate-to-VCA chain settles in
        one tick rather than N.
- [ ] Feedback-patch regression suite: Karplus-Strong, comb
      resonator, FM operator with self-modulation, resonant SVF.
      Each must produce bit-identical output to pre-0848 for at
      least 1 second of synthesis. (The cycle-breaking edge is
      still delayed; nothing should change on the feedback path.)
- [ ] Property test: random DAG generator + module shim that records
      its read order. For every fused input, the producer's
      `process` must have been called earlier in the tick. Failures
      indicate planner ordering bug.
- [ ] `just commit` passes (whole workspace).
- [ ] Existing patches that depended on chain-length offsets (if
      any are found) are migrated to use an explicit `Delay(samples=N)`
      module. Audit the test corpus for such cases.

## Notes

The branchy read path has been measured concern in real-time DSP
contexts but the branch is per-input-port-per-tick, predictable
(the flag is constant after plan adoption), and dwarfed by the
DSP work in any non-trivial module. If profiling later flags it,
phase 3's two-region pool eliminates the branch entirely by sorting
fused vs cyclic into separate index spaces.

A cable feeding both a fused consumer (in the same acyclic
subgraph) and a delayed consumer (in a different SCC) is supported
trivially in this phase: the producer writes to slot `wi`; fused
consumer reads `wi`, delayed reads `1 - wi`. No duplicate writes,
no extra storage. This is the case that goes away in phase 3 (the
producer's slot kind is determined by the maximum requirement
across consumers; cycle-source ports get the pair, scratch-source
ports get a single slot).

Validation: 0848's invariant (producer index < consumer index for
fused cables) is the load-bearing safety check. If the planner
ever emits a fused cable in violation, this phase reads stale
data and produces wrong audio. Keep that invariant's panic — do
not relax it to a warning.

The CLAUDE.md desideratum about order-independence is partially
relaxed in this phase. Update CLAUDE.md to note: order-independence
holds across SCCs but is replaced by topo-order-required *within*
fused subgraphs.
