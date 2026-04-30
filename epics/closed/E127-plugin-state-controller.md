---
id: "E127"
title: Plugin state-management refactor — controller, actions, two-pass rescan
created: 2026-04-30
tickets: ["0757", "0758", "0759", "0760", "0761", "0762", "0763"]
adrs: ["0061"]
---

## Goal

Land ADR 0061: a `Controller` in `patches-plugin-common` that owns the
persistable plugin model, an `Action` enum that is the single mutation
entry point, and a `StateDelta` that drives host callbacks
(`mark_state_dirty`, `request_restart`) and snapshot publication.
Fix the scan-before-compile race on patch reload and split rescan
into a cheap probe + a restart-when-needed apply pass.

## Scope

1. New scaffolding: `Controller`, `Action`, `StateDelta`, `Env` trait.
2. Migrate persistable state (`dsl_source`, `module_paths`, `tap_opts`,
   `window_size`, `file_path`) into `Controller`. Delete `*_requested`
   flags from `GuiState` as each handler moves.
3. Migrate CLAP host events (`Activate`, `StateLoad`) into `Action::*`.
4. Two-pass rescan: probe surfaces ABI/dlopen errors without restart;
   apply restarts only if added/replaced/removed.
5. Audio→main poll-and-synthesise: halt, observer diagnostics, and
   plan adoption surfaced as actions on each main-thread tick.

## Acceptance

- All handler-level mutation goes through `Controller::apply`.
- `mark_state_dirty` is called from exactly one place
  (the shell pump reacting to `StateDelta::persistable_changed`).
- Reload of a patch that depends on an FFI module loads on first try.
- Rescan with no actionable change does not restart the engine.
- Controller has unit tests covering persist-dirty, restart, and
  snapshot-diff behaviour without a CLAP host or live registry.
- `cargo clippy` and `cargo test` pass.

## Out of scope

- Tap-frame channel (stays separate per ADR 0061).
- VST3 / standalone shells (controller is reusable but not built here).
- Hot-swap of live module instances (forbidden by ADR 0044 §3).
