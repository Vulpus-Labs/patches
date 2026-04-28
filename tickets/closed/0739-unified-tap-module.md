---
id: "0739"
title: Unified Tap module; retire TriggerTap
priority: high
created: 2026-04-27
---

## Summary

Replace `AudioTap` and `TriggerTap` with a single `Tap` module. Per
channel it exposes three input ports — `mono_in`, `stereo_in`,
`trigger_in` — and the desugarer wires exactly one based on tap type
(see ADR 0059 §4 table). Two input ports are left disconnected per
channel; they resolve to the existing read-null sinks and cost nothing.

## Acceptance criteria

- [ ] `patches-modules` exposes `Tap`; `AudioTap` and `TriggerTap`
      removed.
- [ ] Desugarer maps each tap-type token to the correct port (mono /
      stereo / trigger) on the unified module.
- [ ] `Tap::tick` writes one scalar (or two, for stereo channels — see
      0740) to the backplane per channel, branchless on tap type.
- [ ] Module registry registers a single `Tap`.
- [ ] All existing `meter` / `osc` / `spectrum` / `gate_led` /
      `trigger_led` patches still work.
- [ ] `cargo test` green across the workspace inner-loop crates
      (`patches-core`, `patches-modules`, `patches-dsp`,
      `patches-engine`).

## Notes

ADR 0059 §4. Slot allocation for stereo channels lands in 0740;
identity / ordering changes land in 0741. Keep this ticket scoped to
the module collapse so the diff is reviewable.
