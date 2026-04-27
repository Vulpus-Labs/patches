---
id: "0722"
title: Event log pane bound to status_log
priority: medium
created: 2026-04-26
epic: "E122"
---

## Summary

Render `GuiState::status_log` as a scrolling event-log pane in the
webview. Newest entries last, line formatting mirrors the TUI's
`LogEntry` style.

## Acceptance criteria

- [ ] Diagnostics tab includes an event log section.
- [ ] Auto-scrolls to bottom on new entries unless the user has
      scrolled up.
- [ ] Lines formatted with a leading `HH:MM:SS` UTC stamp matching
      `patches-player/src/tui.rs`.
- [ ] Capped at `STATUS_LOG_CAPACITY` entries (existing constant).
- [ ] `cargo clippy` and `cargo test` clean.
