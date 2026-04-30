---
id: "0759"
title: Migrate UI handlers to Action::* and delete *_requested flags
priority: medium
created: 2026-04-30
epic: E127
adrs: ["0061"]
---

## Summary

Replace the polling-via-bool flow in `on_main_thread` with an action
queue. Webview intents deserialise straight to `Action`; the main
thread drains the queue and calls `Controller::apply`. Each delta's
`persistable_changed` / `requires_restart` flags drive
`mark_state_dirty` / `request_restart`. The four ad-hoc
`mark_state_dirty` call sites added in 0754 collapse to one.

## Acceptance criteria

- [ ] Migrate `Browse`, `Reload`, `LoadPath` (new), `AddModulePath`,
      `RemoveModulePath`, `Rescan`, `SetTapOpts` to `Action`.
- [ ] Delete `browse_requested`, `reload_requested`,
      `add_path_requested`, `remove_path_index`, `rescan_requested`
      from `GuiState`.
- [ ] Action queue is `Mutex<VecDeque<Action>>` on the plugin, drained
      each `on_main_thread` tick.
- [ ] `mark_state_dirty` called from exactly one place (shell pump).
- [ ] Status messages and diagnostic clearing happen inside controller
      handlers, not in the shell.
- [ ] Unit tests on the controller cover the per-action delta flags.

## Notes

Webview JSON shapes stay the same — `Intent` is renamed to `Action`
and gains variants but the existing kinds round-trip.
