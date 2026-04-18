---
id: "0515"
title: Slim patches-engine to backend-agnostic kernel
priority: medium
created: 2026-04-17
---

## Summary

With planner extracted (0513) and cpal extracted (0514),
`patches-engine` should contain only the kernel, executor, module
pool, execution state, processor, and backend-agnostic engine shell.
Audit what remains and remove anything that has crept in.

Part of epic E089 (see ADR 0040). Depends on 0513 and 0514.

## Acceptance criteria

- [x] `patches-engine/src/` contains only: `lib.rs`, `kernel.rs`,
  `execution_state.rs`, `pool.rs`, `processor.rs`, `decimator.rs`,
  `oversampling.rs`, `cleanup.rs`, `midi/`. No `engine.rs` — it was
  fully eliminated during prior waves; the kernel is the engine.
- [x] No `cpal`, `notify`, file I/O, or DSL-parsing deps in
  `patches-engine/Cargo.toml`.
- [x] `patches-engine` depends on: `patches-core`, `patches-dsp`,
  `patches-planner`, `patches-registry`, plus `rtrb` (lock-free ring
  buffer for cleanup channel) and `midir` (MIDI routing kept in engine
  per 0514 decision).
- [x] `cargo tree -p patches-engine` shows minimal kernel dep set.
- [x] All engine unit and integration tests pass (17 unit + 3 planner).
- [x] `cargo build`, workspace `cargo test` (1394 passed, 0 failed),
  `cargo clippy` clean for engine (pre-existing warnings in dsp/tracker).

## Notes

This ticket is primarily a verification pass after 0513 and 0514; most
of the actual moves happen in those two. Use this ticket to sweep up
anything that was missed, tighten `Cargo.toml`, and confirm the
dependency surface matches the intent in ADR 0040.

If MIDI routing (`patches-engine/src/midi/`) is found to be
cpal-coupled during 0514, it will have been moved; if it was kept, it
stays here. Either way, document the final state in the PR.

## Outcome

Verification pass — no code changes needed. Wave 1/2 (0512-0514, 0519)
left `patches-engine` already at minimal surface:

- src/: `lib.rs`, `kernel.rs`, `execution_state.rs`, `pool.rs`,
  `processor.rs`, `decimator.rs`, `oversampling.rs`, `cleanup.rs`,
  `midi/`.
- Cargo deps: `patches-core`, `patches-registry`, `patches-planner`,
  `patches-dsp`, `rtrb`, `midir`.
- `lib.rs` retains temporary planner re-exports (flagged for downstream
  migration); not in scope of this ticket.
- LSP gate: `cargo tree -p patches-lsp` shows no `cpal`, `engine`, or
  `planner` transitively.
