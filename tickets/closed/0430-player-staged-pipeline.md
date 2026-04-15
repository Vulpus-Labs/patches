---
id: "0430"
title: Migrate patches-player to staged pipeline
priority: medium
created: 2026-04-15
---

## Summary

Replace `patches-player/src/main.rs::load_patch()` and the hot-reload
loop (`run()` around lines 232–270) with calls to the orchestrator
from 0428 under a fail-fast policy. Each stage failure should produce
a stage-scoped error message rendered via `patches-diagnostics`.

## Acceptance criteria

- [ ] `load_patch` replaced by an orchestrator call; first failing
      stage's diagnostics are printed and the load is aborted.
- [ ] Hot-reload re-runs the full pipeline on file change; dependency
      set for the watcher still comes from `LoadResult.dependencies`.
- [ ] No behavioural change for the success path.
- [ ] Error-path tests exercise stage-1 through stage-5 failures.
- [ ] `cargo test -p patches-player`, `cargo clippy` clean.

## Notes

Depends on E079. Scope excludes incremental caching within a single
watcher tick — that's a separate optimisation and not required by
ADR 0038.
