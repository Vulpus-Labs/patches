---
id: "0705"
title: TUI subscribes to observer; per-tap drop counters in event log
priority: high
created: 2026-04-26
---

## Summary

Wire the patches-player TUI (ticket 0704) to the observer's
latest-scalar surface (ticket 0701). Meter pane reads peak + RMS per
slot at ~30 Hz; the event log surfaces drop counters and pipeline
diagnostics ("not yet implemented" for unsupported tap types).
Closes the end-to-end loop for E119: a `~meter(...)` declaration in
a `.patches` file produces visible bars in the player.

## Acceptance criteria

- [ ] TUI redraw loop reads from observer subscriber surface in place
  of the stub data source from ticket 0704.
- [ ] Meter pane labels each bar pair by tap name (sourced from the
  active manifest).
- [ ] Drop counters surface in the event log when they advance, with
  rate-limiting so a slow observer doesn't spam the log. Slot →
  tap-name resolution uses the latest manifest snapshot the observer
  saw.
- [ ] Pipeline diagnostics (`osc` / `spectrum` / `gate_led` /
  `trigger_led` declared in the patch) appear once per manifest
  publication in the event log: "tap `<name>` (`<type>`): not yet
  implemented".
- [ ] Reload of a patch with different taps: the meter pane updates
  to the new tap set without a restart; old per-slot state on
  preserved names survives the reload.
- [ ] End-to-end test: a fixture patch with two `~meter(...)` taps
  loads, runs in the player against a deterministic test signal,
  and reaches a steady-state where the displayed peak/RMS values
  match expected within a tolerance.

## Notes

This ticket is the "make it real" milestone. Once it ships, the
reference frontend principle from ADR 0055 is in force:
observation-related changes land in `patches-player` first, plugin
shell adopts later.

Slow-observer test: induce ring overruns by sleeping the consumer;
assert drop counters increment and surface in the event log.

## Cross-references

- ADR 0053 §7 — latest-scalar surface.
- ADR 0055 §6 — observer → UI dispatch.
- Tickets 0700, 0701, 0704 — the surfaces this ticket joins together.
- E119 — parent epic.
