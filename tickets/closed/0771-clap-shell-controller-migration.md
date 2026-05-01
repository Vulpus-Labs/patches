---
id: "0771"
title: Migrate patches-clap to Controller / Action / Env
priority: high
created: 2026-04-30
---

## Summary

Route every JSON intent and CLAP host callback in `patches-clap`
through `Controller::apply`. Implement a CLAP `Env` covering file
dialogs (via wry/rfd on the main thread), DSL compile + plan dispatch
through the existing engine handle, and no-op sidecar methods.
Completes ADR 0061 steps 2–6 for the CLAP shell.

## Acceptance criteria

- [ ] All webview JSON intents in `patches-clap` lower to `Action`
      variants and call `Controller::apply`. No direct mutation of
      shell state outside the controller.
- [ ] All CLAP host callbacks (`activate`, `deactivate`, `state_save`,
      `state_load`) lower to `Action` or read `Controller`/snapshot.
- [ ] CLAP `Env` impl lives in `patches-clap`; no CLAP types leak into
      `patches-plugin-common`.
- [ ] `state_save` / `state_load` round-trips `SerializedState`
      (current shape; schema changes land in 0774).
- [ ] Snapshot push to webview goes through `Controller::snapshot()`.
- [ ] `cargo clippy -p patches-clap` and existing CLAP tests pass.

## Notes

ADR 0061 §6, ADR 0063 §1, §7 step 3. Persistence-shape changes are
out of scope — keep `SerializedState` as-is; 0773 and 0774 reshape it.
