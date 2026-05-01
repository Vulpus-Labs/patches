# E131 — Per-instance CPU monitoring

**Status:** Open
**Created:** 2026-05-01
**ADR:** [0065 — Per-instance CPU monitoring](../../adr/0065-per-instance-cpu-monitoring.md)

## Goal

End-to-end per-instance CPU cost monitoring: planner → engine
(timed dispatch) → observer (aggregation) → ratatui UI tab. Zero
overhead when disabled.

## Tickets

- [x] 0778 — Route instance names from planner to observer with drop
  ladder
- [x] 0779 — Optional timed dispatch in engine, raw block records on
  SPSC
- [x] 0780 — Ratatui CPU-monitor tab in patches-player

## Out of scope

See ADR 0065 "Out of scope" section: per-cable timing, sub-block
resolution, runtime monitor reconfiguration, non-time cost
counters.
