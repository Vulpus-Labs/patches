---
id: "0761"
title: Move tap_opts and window_size into Controller; delete GuiState mirrors
priority: medium
created: 2026-04-30
epic: E127
adrs: ["0061"]
---

## Summary

Final field migration. `tap_opts` and `window_size` become controller-
owned. Webview intents that mutated `GuiState` directly on the webview
thread now post `Action::SetTapOpts` / `Action::SetWindowSize` to the
queue. `GuiSnapshot::from_state` becomes `Controller::snapshot`;
`GuiState` either disappears or shrinks to `status_log` +
`diagnostic_view` + `halt` + `taps` (derived live state).

## Acceptance criteria

- [ ] `Controller` owns `tap_opts` and `window_size`.
- [ ] Webview no longer mutates shared state directly; all paths post
      actions.
- [ ] `Action::SetTapOpts` / `SetWindowSize` set
      `persistable_changed = true`, so Reaper-style hosts dirty.
- [ ] `GuiSnapshot::from_state` removed in favour of
      `Controller::snapshot()`.
- [ ] No `Mutex<GuiState>` lock on the webview thread for snapshot
      shape data (status_log etc. may keep a lock if needed).

## Notes

This is the ticket that closes the "tap-opts changes don't dirty
state" gap noted in ADR 0061.
