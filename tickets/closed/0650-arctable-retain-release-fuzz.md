---
id: "0650"
title: ArcTable retain/release randomised fuzz
priority: high
created: 2026-04-23
epic: "E111"
---

## Summary

Randomised retain/release sequence fuzz against `ArcTable`. Check
invariants: no double-free, refcount monotone-to-zero per slot, no
leaks at sequence end, capacity growth events consistent.

## Acceptance criteria

- [ ] Proptest (or equivalent) stressing interleaved retain/release
      across multiple slots and capacities.
- [ ] Invariant checkers: final refcount zero after balanced
      sequences; zero leaks; no aliasing across freed slots.
- [ ] Runs green under Miri or loom where feasible; otherwise
      documents gap.
- [ ] CI time-boxed; nightly longer.

## Notes

ADR 0045 §Spike 9. Spike 6 established chunked storage + RCU index
swap; this ticket verifies its runtime behaviour under adversarial
sequences.
