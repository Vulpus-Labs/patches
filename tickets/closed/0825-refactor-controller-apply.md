---
id: "0825"
title: Split controller `apply` (275 LOC, cognitive 27)
priority: medium
created: 2026-05-06
---

## Summary

`PluginController::apply` in [patches-plugin-common/src/controller.rs:380](../../patches-plugin-common/src/controller.rs#L380)
trips three readability lints simultaneously: `too_many_lines` (275/150),
`cognitive_complexity` (27/25), and four `excessive_nesting` hits inside it.
It is the single largest readability outlier in the workspace.

The function dispatches on `Action` and mutates controller state in-place.
Each arm is independently understandable but the whole function is too dense
to scan. Split per-arm handlers (free fns or `impl` methods) returning the
`StateDelta` slice they produce; `apply` becomes a thin match.

## Acceptance criteria

- [ ] `apply` ≤ 60 LOC; each extracted handler ≤ 80 LOC
- [ ] No clippy warnings from `too_many_lines`, `cognitive_complexity`, or
      `excessive_nesting` for this function under the workspace `clippy.toml`
- [ ] Existing controller tests pass; no behaviour change
- [ ] `just commit -p patches-plugin-common` clean

## Notes

Lint thresholds in `clippy.toml`: too-many-lines=150, excessive-nesting=5,
cognitive-complexity=25.
