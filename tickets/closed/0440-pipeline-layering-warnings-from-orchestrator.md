---
id: "0440"
title: Emit layering warnings from pipeline orchestrator
priority: high
created: 2026-04-15
---

## Summary

`RenderedDiagnostic::pipeline_layering_warnings()` (PV0001, ticket 0437)
is currently called only from `patches-lsp/src/workspace.rs:876`. Player
(`patches-player/src/main.rs:91-130`) and CLAP
(`patches-clap/src/plugin.rs:195-208`) never emit it, so pipeline
layering violations are silently dropped for two of three consumers.

Move the emission into `patches-dsl::pipeline::run_all` and
`run_accumulate` so it runs once, at the pipeline boundary, for every
consumer. Warnings become part of the `Staged` / `AccumulatedRun`
result rather than a per-consumer opt-in.

## Acceptance criteria

- [ ] `Staged<T>` and `AccumulatedRun<T>` expose a unified
      `warnings: Vec<RenderedDiagnostic>` that includes PV#### layering
      warnings alongside expand warnings.
- [ ] `run_all` and `run_accumulate` invoke
      `pipeline_layering_warnings()` after bind and fold the result in.
- [ ] Player, CLAP, and LSP read warnings from the pipeline result; no
      consumer calls `pipeline_layering_warnings()` directly.
- [ ] Integration test: a patch that triggers PV0001 produces the same
      warning diagnostic from all three consumers.
- [ ] `cargo test`, `cargo clippy` clean.

## Notes

Part of E082. Depends on 0439 being in flight (shared converter path)
but does not strictly block on it. The warning type needs to be broad
enough that future PV#### codes (ticket 0437 left room) can be added
without a signature change — prefer `Vec<RenderedDiagnostic>` over a
narrow enum.
