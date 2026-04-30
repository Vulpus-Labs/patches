---
id: "0757"
title: Add Controller / Action / StateDelta / Env scaffolding
priority: medium
created: 2026-04-30
epic: E127
adrs: ["0061"]
---

## Summary

Land the new types in `patches-plugin-common` with no callers. Pure
addition: existing `GuiState` and `Intent` keep working unchanged.

## Acceptance criteria

- [ ] `Controller` struct with persistable + derived fields per ADR 0061.
- [ ] `Action` enum covering current `Intent` variants plus `Activate`,
      `StateLoad`, `HaltObserved`, `PlanAdopted`, `DiagnosticsDrained`.
- [ ] `StateDelta { persistable_changed, requires_restart,
      snapshot_changed, plan_recompile }`.
- [ ] `Env` trait with `read_file`, `pick_file`, `pick_folder`,
      `compile_and_push_plan`, `scan_paths`, `probe_paths`.
- [ ] `Controller::apply` stub that compiles for every `Action`
      variant (returns `StateDelta::default()` for now).
- [ ] `Controller::snapshot()` produces the existing `GuiSnapshot` shape.
- [ ] Unit tests: snapshot round-trip, default delta on no-op actions.

## Notes

No behaviour change yet. The CLAP plugin still drives state through
`GuiState` and `Intent`. Subsequent tickets migrate handlers one at a
time.
