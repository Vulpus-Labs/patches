---
id: "0275"
title: Implement vizia UI with labels, buttons, and state synchronisation
priority: high
created: 2026-04-07
---

## Summary

Build the vizia UI that replicates the current plugin interface: a path label,
Browse and Reload buttons, and a status label. Wire the vizia event system to
the existing `GuiState` so that button clicks set the request flags and label
text updates when state changes.

## Acceptance criteria

- [ ] vizia UI displays a path label showing the loaded file path (or
      "No file loaded").
- [ ] Browse button sets `gui_state.browse_requested` and triggers
      `host->request_callback()`.
- [ ] Reload button sets `gui_state.reload_requested` and triggers
      `host->request_callback()`.
- [ ] Status label displays the current `gui_state.status` string.
- [ ] `ViziaGuiHandle::update(&self, state: &GuiState)` refreshes label text
      in the vizia context.
- [ ] The UI has reasonable styling (margins, spacing, readable font size).
- [ ] `cargo clippy -p patches-clap` passes with no warnings.

## Notes

- The `GuiState` struct and the `on_main_thread` callback pattern in
  `plugin.rs` should not need changes — the vizia UI replaces only the
  rendering/input layer.
- `rfd` remains the file dialog implementation; it is already cross-platform.
- Depends on T-0274.
