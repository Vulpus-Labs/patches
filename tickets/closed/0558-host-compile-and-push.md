---
id: "0558"
title: Collapse HostRuntime::compile + push_plan
priority: medium
created: 2026-04-18
---

## Summary

`HostRuntime::compile` returns `(LoadedPatch, ExecutionPlan)`
(`patches-host/src/builder.rs:127-140`), forcing every caller to
destructure and then call `push_plan` separately. Both current
consumers do exactly this. Collapse to `compile_and_push(&source,
&registry) -> Result<LoadedPatch, CompileError>` and keep the plan
internal. If a consumer genuinely needs the plan (none currently do),
expose `last_plan(&self) -> Option<&ExecutionPlan>`.

Part of epic E093.

## Acceptance criteria

- [ ] `HostRuntime::compile_and_push` exists and is the documented
      entry point.
- [ ] Old `compile` either removed or kept as internal helper; no
      longer pub.
- [ ] `patches-player` and `patches-clap` updated; no tuple
      destructure at call sites.
- [ ] Hot-reload path in both consumers still works (manual smoke test
      noted in PR description).

## Notes

Depends on 0556 (private fields). Land before 0559 so the error-path
changes stack cleanly.
