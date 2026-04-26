---
id: "0702"
title: Manifest plumbing — planner → observer control ring
priority: high
created: 2026-04-26
---

## Summary

Wire the `Vec<TapDescriptor>` manifest emitted in E118 ticket 0697
through the planner to the observer over the planner→observer
control ring (ADR 0053 §6). Planner injects `sample_rate` (host rate
× oversampling) at build time. Observer keys per-slot pipeline state
by tap name; on slot shifts a one-frame meter blip is acceptable
(ADR 0054 §3).

## Acceptance criteria

- [ ] Planner-side: at the same point it ships a new module graph,
  also publish the current manifest with `sample_rate` filled in.
- [ ] Control ring carrying manifest publications uses an existing
  lock-free SPSC primitive; do not invent a new one.
- [ ] Observer-side: drain manifest publications between frame
  batches; rebuild per-slot pipeline state keyed by tap name. Names
  preserved across publications keep their state; names that are new
  start fresh; names that disappear release state.
- [ ] Slot shift handling: when the audio thread adopts a new graph
  before the observer drains the corresponding manifest, frames may
  briefly land on a slot the observer's old manifest pointed at a
  different name. Document and accept the one-frame blip; do not add
  cross-thread synchronisation.
- [ ] Unit + integration tests: publish, modify, and remove taps;
  assert state preservation by name and correct slot mapping.

## Notes

This ticket is mostly plumbing. The hard parts (audio side, observer
runtime) live in 0699/0700/0701; here we only need to thread the
manifest through and verify name-keyed state survives renames.

## Cross-references

- ADR 0053 §6 — planner→observer control ring.
- ADR 0054 §3 — slot ordering and rename behaviour.
- ADR 0054 §6 — manifest shape.
- E118 ticket 0697 — manifest emission.
- E119 — parent epic.
