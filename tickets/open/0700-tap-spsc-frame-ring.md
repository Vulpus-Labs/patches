---
id: "0700"
title: SPSC frame ring (audio → observer)
priority: high
created: 2026-04-26
---

## Summary

Allocate the SPSC ring carrying backplane snapshots from the audio
thread to the observer thread per ADR 0053 §5. Per-block: write a
chunk of frames, commit, and on full-ring drop with a per-slot
counter increment. Audio side never blocks.

## Acceptance criteria

- [ ] Lock-free SPSC ring (e.g. `rtrb` or equivalent already used in
  the engine) sized per ADR 0053 §5 in number of frames.
- [ ] Frame layout: one `[f32; MAX_TAPS]` snapshot per audio sample,
  packed contiguously per block. Decision recorded in ticket on close
  if implementation deviates (e.g. block-of-blocks vs sample-stream).
- [ ] Audio side: end-of-block commit; if the ring can't fit the
  block, drop and increment a per-slot drop counter (`AtomicU64`).
  Counters are observable from the observer thread via a stable API.
- [ ] No allocation on the audio thread; ring is pre-allocated at
  planner build time.
- [ ] Observer side: `drain()` returns an iterator over available
  frames without copying.
- [ ] Unit tests: writer/reader on synthetic threads; assert
  ordering, drop-on-full behaviour, counter increments.

## Notes

Drop policy is intentional: prefer dropping over blocking the audio
thread. UI surfaces drop counters in the event log (ticket 0705) so
observation gaps are user-visible.

If a block-of-blocks layout (one ring entry = one block) is simpler
than per-sample frames, choose that — observer pipelines work the
same way. Document the choice in code.

## Cross-references

- ADR 0053 §5 — ring sizing rationale.
- E119 — parent epic.
