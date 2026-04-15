---
id: "0431"
title: Map patches-clap CompileError onto pipeline stages
priority: medium
created: 2026-04-15
---

## Summary

`patches-clap::error::CompileError` already discriminates
`Load|Parse|Expand|Interpret|Plan`. With the stage split from E079,
`Expand` needs to become `Expand` + `Structural`, and
`load_or_parse()` / `compile_and_push_plan()` in `plugin.rs` should
call the orchestrator under a fail-fast policy.

## Acceptance criteria

- [ ] `CompileError` gains a `Structural` variant distinct from
      `Expand`; existing `Expand` narrows to mechanical expansion.
- [ ] `load_or_parse` and `compile_and_push_plan` call the
      orchestrator; no direct calls to individual DSL/interpreter
      functions remain.
- [ ] Plugin surface unchanged; hot-reload still pushes a new plan on
      successful compile.
- [ ] Error-path tests cover each variant.
- [ ] `cargo test -p patches-clap`, `cargo clippy` clean.

## Notes

Depends on E079, 0430 (for shared orchestrator usage patterns).
