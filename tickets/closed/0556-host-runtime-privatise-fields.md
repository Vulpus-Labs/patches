---
id: "0556"
title: Privatise HostRuntime fields
priority: medium
created: 2026-04-18
---

## Summary

`HostRuntime` exposes `planner`, `plan_tx`, and `env` as public fields
(`patches-host/src/builder.rs:92-104`). Consumers can bypass
`push_plan`'s error handling by sending on `plan_tx` directly, or
mutate planner state out from under `compile`. Make fields private;
add accessors only where a consumer actually needs read access.

Part of epic E093.

## Acceptance criteria

- [ ] `HostRuntime` fields private.
- [ ] Read-only accessors: `env(&self) -> &AudioEnvironment`,
      `planner(&self) -> &Planner` (only if a current consumer
      reads it; otherwise omit).
- [ ] All plan-pushing goes through `push_plan` (or the collapsed
      API from 0558).
- [ ] `patches-player` and `patches-clap` compile and tests pass.

## Notes

Scope: field access only. Do not reshape the struct or split types
here — 0557 handles the module layout.
