---
id: "0799"
title: Tiered validation profiles (inner / commit / push / smoke)
priority: high
created: 2026-05-03
epic: E133
---

## Summary

Define four named validation tiers — `inner`, `commit`, `push`,
`smoke` — as Justfile recipes (or cargo aliases) so the agent's inner
loop pays only for fast feedback, and slower checks (clippy,
integration, plugin scanning) run on a coarser cadence. See
[ADR 0067](../../adr/0067-blast-radius-cuts-within-monorepo.md) §2.

## Acceptance criteria

- [ ] `Justfile` (or equivalent) at repo root with recipes:
  - `inner` — `cargo test` on inner-loop crate set (`patches-core`,
    `patches-modules`, `patches-dsp`, `patches-engine`) plus the
    crate(s) passed as args; no clippy.
  - `commit` — `inner` + `cargo clippy` on the same scope.
  - `push` — full workspace `cargo build`, `cargo test`, `cargo
    clippy`, forbidden-edge lint (when 0800 lands).
  - `smoke` — `push` + integration tests, plugin scanner, LSP, CLAP,
    slow suites.
- [ ] CLAUDE.md documents which tier the agent runs at which moment
      (per-iteration: `inner`; pre-commit: `commit`; pre-push: `push`).
- [ ] GitHub Actions workflow runs `just push` on push to main and on
      PRs. `just smoke` wired to a separate trigger (manual dispatch
      and/or schedule).
- [ ] Verify timings: `inner` should finish in seconds for typical
      changes; `push` is allowed to be slow.

## Notes

Static tiers chosen over a dynamic reverse-dep walk; see ADR 0067 for
rationale. If the workspace grows or contributor count goes up,
revisit.

`inner` excludes plugin scanner, CLAP, LSP per existing inner-loop
memory. Touched crate added via `just inner patches-svg` or similar.
