---
id: "0772"
title: Migrate patches-player Ratatui to Controller + Ratatui Env
priority: high
created: 2026-04-30
---

## Summary

Replace bespoke TUI state in `patches-player` with `Controller`.
Implement a Ratatui `Env`: file picker (rfd or CLI arg fallback),
file read, DSL compile + plan dispatch through the existing engine
handle, real sidecar load/save methods (sidecar lifecycle wired in
0776).

Tap rendering already consumes the subscriber surface and does not
move; this ticket is state-management only.

## Acceptance criteria

- [ ] `patches-player` holds a `Controller` and routes keystrokes
      through `Action::*`.
- [ ] Ratatui `Env` impl lives in `patches-player`; no ratatui types
      in `patches-plugin-common`.
- [ ] Snapshot drives all rendering of file path, status log,
      diagnostics, module paths.
- [ ] Existing keybindings (load, reload, rescan, tap config) work
      via the controller path.
- [ ] `cargo clippy -p patches-player` and existing player tests pass.

## Notes

ADR 0063 §1, §7 step 4. Sidecar lifecycle is 0776; this ticket only
needs the `Env` methods to exist (returning empty / Ok(())).
