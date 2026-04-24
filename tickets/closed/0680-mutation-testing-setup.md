---
id: "0680"
title: Mutation testing setup (cargo-mutants + config)
priority: medium
created: 2026-04-24
epic: E117
---

## Summary

Install `cargo-mutants`, add workspace config excluding non-kernel
crates and test files, verify baseline run works on one small crate.

## Acceptance criteria

- [ ] `cargo install cargo-mutants` documented in epic notes.
- [ ] `.cargo/mutants.toml` added with exclude globs for clap/lsp/ffi/
      bin/vscode/player/modules/io/profiling/integration-tests and
      `**/tests.rs` / `**/tests/**`.
- [ ] `timeout_multiplier = 2.0` (or justified alternative).
- [ ] Smoke run on `patches-core` completes and produces `mutants.out/`.
- [ ] Invocation examples recorded in E117 notes.

## Notes

Use inner-loop test subset baseline per auto-memory: skip
plugin-scanner / clap / lsp.
