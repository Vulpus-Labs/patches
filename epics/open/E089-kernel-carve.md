---
id: "E089"
title: Carve stable kernel — registry, planner, cpal, host crates
created: 2026-04-17
tickets: ["0512", "0513", "0514", "0515", "0516", "0517", "0518", "0519"]
---

## Goal

Carve four new crates out of the workspace and slim `patches-engine` so
the core execution machinery becomes a bounded, publishable kernel that
external plugins, out-of-tree LSP/SVG, and future hosts can depend on
without dragging cpal or unrelated concerns.

After this epic:

- `patches-registry` owns module registration and plugin-loading
  surface; `patches-core` is registry-agnostic.
- `patches-planner` owns `Planner`, `ExecutionPlan`, and
  `PlannerState`; `ModuleGraph` stays in `patches-core`.
- `patches-cpal` owns the cpal stream and audio callback;
  `patches-engine` becomes backend-agnostic.
- `patches-host` owns the composition shared by player and CLAP
  (registry init, pipeline driving, planner construction,
  processor spawn) behind `HostFileSource` / `HostAudioCallback`
  traits.
- `patches-player` and `patches-clap` shrink to their integration
  layers.
- `patches-lsp` depends on interpreter + registry + dsl only —
  no planner, no engine, no cpal.

## Background

See ADR 0040 for the full rationale: prepare for monorepo breakup,
narrow focus and lower blast-radius for changes, support future
dynamic plugin loading in LSP and CLAP, and align crate boundaries
with the parse / bind / build / plan / execute phases they represent.

ADR 0012 (planner v2 graph-diffing, status *proposed*) overlaps with
the planner extraction; coordinate so we do not carve twice.

## Tickets

| ID   | Title                                               | Priority | Depends on       |
| ---- | --------------------------------------------------- | -------- | ---------------- |
| 0512 | Extract patches-registry from patches-core          | high     | —                |
| 0513 | Extract patches-planner and move PlannerState       | high     | 0512             |
| 0514 | Extract patches-cpal from patches-engine            | medium   | —                |
| 0515 | Slim patches-engine to backend-agnostic kernel      | medium   | 0513, 0514       |
| 0519 | Narrow FlatPatch/BoundPatch via SongData split      | medium   | —                |
| 0516 | New patches-host crate with shared composition      | medium   | 0513, 0515, 0519 |
| 0517 | Port patches-player to patches-host + patches-cpal  | medium   | 0514, 0516       |
| 0518 | Port patches-clap to patches-host                   | medium   | 0516             |

Execution order:

1. 0512 (registry), 0514 (cpal), 0519 (SongData split) — all
   independent; can run in parallel.
2. 0513 (planner) — needs 0512 for the `&Registry` import path.
3. 0515 (slim engine) — needs 0513 and 0514 out so engine is left
   with just the kernel.
4. 0516 (host) — needs planner, slim engine, and the narrowed patch
   types so the shared composition lands on the clean shape rather
   than freezing the current dual-arg pairing.
5. 0517 / 0518 (consumers) — final port; can land in either order but
   both block closing the epic.

## Definition of done

- All four new crates compile as library targets with `publish = false`
  (published later if and when kernel externalization happens).
- `patches-core` no longer exposes registry types; `patches_core::Registry`
  has no callers.
- `patches-engine` contains no cpal dependency and no planner module.
- `patches-player` and `patches-clap` call through `patches-host`
  rather than duplicating composition.
- `patches-lsp`'s `Cargo.toml` contains no transitive dependency on
  `patches-engine`, `patches-planner`, or `cpal` (asserted via
  `cargo tree -p patches-lsp`).
- `cargo build`, `cargo test`, `cargo clippy` clean workspace-wide.
- No `unwrap()`/`expect()` added to library code during the moves.
- All existing integration tests pass unchanged (this is a
  boundary-only refactor; no behaviour change).
