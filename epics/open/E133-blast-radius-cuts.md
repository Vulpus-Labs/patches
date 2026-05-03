---
id: E133
title: Blast-radius cuts within the monorepo
status: open
created: 2026-05-03
---

## Goal

Reduce the blast radius of changes within the workspace: a change to
`patches-modules` should not retest `patches-svg`; a change to a leaf
binary should retest nothing else. Harden the dep graph so the cuts
don't rot. Prepare ground for a future repo split without committing
to one.

See [ADR 0067](../../adr/0067-blast-radius-cuts-within-monorepo.md)
for rationale and approach.

## Tickets

- 0798 — Split `patches-svg` into lib + `patches-svg-cli`; drop
  `patches-modules` from the lib.
- 0799 — Tiered validation profiles (`inner` / `commit` / `push` /
  `smoke`) so the agent's inner loop runs only fast checks and
  heavier ones run on coarser cadences.
- 0800 — Forbidden-edge dep-graph lint (`cargo deny` or `cargo
  metadata` walk in CI) codifying the cuts.
- 0801 — `patches-tools` lib/bin split (deferred; track when pressure
  warrants).

## Done when

- `patches-modules` churn no longer retests `patches-svg` (lib) or
  `patches-lsp`.
- A leaf binary change's CI retests only that crate.
- Forbidden edges fail CI rather than landing silently.
- `patches-svg-cli` exists; the renderer lib has no module deps.
