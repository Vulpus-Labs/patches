---
id: "0559"
title: CompileError carries SourceMap
priority: high
created: 2026-04-18
---

## Summary

The one remaining abstraction leak across `patches-host`: both
`patches-player` (`main.rs:30-37`) and `patches-clap`
(`plugin.rs:129-133`) re-derive a `SourceMap` when rendering a
`CompileError`. Player even re-calls `patches_dsl::pipeline::load` to
recover it — the single point where player reaches past
`patches-host` into the DSL crate. Fix at the source: bake the
`SourceMap` into `CompileError` (or expose
`CompileError::to_diagnostics(&registry) -> Vec<Diagnostic>` from
`patches-host`) so consumers never need to re-derive it.

Part of epic E093.

## Acceptance criteria

- [ ] `CompileError` carries enough context (source map, or a
      rendered-diagnostics accessor) that consumers do not need to
      reload source to render errors.
- [ ] `patches-player/src/main.rs` no longer imports
      `patches_dsl::pipeline` — the last direct DSL reach-through is
      gone.
- [ ] `patches-clap` error-render path no longer reconstructs
      `last_source_map`.
- [ ] Diagnostic output identical before/after (snapshot test if
      feasible, otherwise manual check on a known-bad patch).

## Notes

Highest-value ticket in E093: kills the one real leak. Do after 0558
so the API is stable before we extend `CompileError`.
