---
id: "0725"
title: Route observer Diagnostic items into status_log
priority: medium
created: 2026-04-26
epic: "E122"
---

## Summary

Forward `patches_observation::subscribers::Diagnostic` items
(`NotYetImplemented`, `InvalidSlot`) into `GuiState::status_log` so
unsupported tap declarations are visible in the webview. Format
matches `patches-player/src/tui.rs::format_diagnostic`.

## Acceptance criteria

- [ ] On each `on_main_thread` tick, observer diagnostics are drained
      and pushed to `status_log` via `push_status`.
- [ ] Format matches the TUI output verbatim.
- [ ] No duplicate spamming — diagnostics are only pushed once per
      occurrence.
- [ ] `cargo clippy` and `cargo test` clean.
