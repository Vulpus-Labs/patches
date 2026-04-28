---
id: "0735"
title: Reduce ModuleShape to { channels }
priority: high
created: 2026-04-28
epic: "E126"
adrs: ["0060"]
depends_on: ["0734"]
---

## Summary

Remove `length` and `high_quality` from `ModuleShape`. After this
ticket `ModuleShape` carries `channels: usize` only. The two removed
fields are reintroduced as structural params on the modules that
actually consume them (handled in 0736 and 0737); during this ticket
the consumers temporarily hard-code defaults so the workspace
compiles green.

## Acceptance criteria

- [ ] `ModuleShape` definition reduces to `{ pub channels: usize }`.
- [ ] Every `ModuleShape { channels: _, length: _, high_quality: _ }`
      literal across the workspace simplifies to
      `ModuleShape { channels: _ }`. Most call sites become much
      shorter.
- [ ] `ModuleShape::default()` returns `{ channels: 0 }`.
- [ ] `pitch_shift`, `delay`, `stereo_delay` temporarily hard-code the
      former `shape.length` / `shape.high_quality` values to defaults
      (`length: 0`, `high_quality: false`). Comment with the ticket
      number that will reintroduce them as structural params.
- [ ] `param_layout::hash` updated if it relied on the removed fields
      (it shouldn't — they didn't shape ports — but verify).
- [ ] `cargo test` and `cargo clippy` pass.

## Notes

Touches: `patches-core/src/modules/module_descriptor.rs`,
`patches-core/src/test_support/harness/`, every module in
`patches-modules/src/`, fixtures in `patches-engine/tests/`,
`patches-integration-tests/`. Mechanical, but ~50+ files.

Use `cargo fmt` after to clean up the now-shorter literals.
