---
id: "0780"
title: Ratatui CPU-monitor tab in patches-player
priority: low
created: 2026-05-01
---

## Summary

When patches-player is started with monitoring enabled, add a
ratatui tab showing per-instance CPU cost as a percentage of block
budget, sorted descending. No tab when monitoring disabled.

See [ADR 0065](../../adr/0065-per-instance-cpu-monitoring.md).

## Acceptance criteria

- [ ] CLI flag (e.g. `--monitor` or `--cpu-monitor`) on
  `patches-player` that enables engine-side `MonitorConfig` and
  registers an observer.
- [ ] Observer aggregates `MonitorBlock` records into a per-slot
  rolling estimate:
  `est_cost = module_accum * block_samples / module_samples_timed`
  averaged over a window (window length documented; e.g. 1 s).
- [ ] On `PlanMeta` receipt, observer rebuilds slot → name table;
  resets per-slot estimates not preserved by the new mapping.
- [ ] Ratatui adds a "CPU" tab when monitoring enabled. Tab shows:
  instance name, type name (if available), % of block budget,
  optional sparkline. Sorted by % desc.
- [ ] Tab absent when monitor disabled — zero UI cost in the
  default path.
- [ ] Periodic-phase cost shown as a separate aggregate row
  (or section).
- [ ] Block duration / total measured % shown so user can see
  unattributed time.

## Notes

- Type-name aggregation is observer-side and optional for v1; can
  ship with instance-only view first.
- Sparkline is nice-to-have. Plain numbers are fine for v1.
- Convergence: with N modules round-robin, each gets
  ~(window / N) blocks of timed data. Document expected settling
  time in the tab header or tooltip.
