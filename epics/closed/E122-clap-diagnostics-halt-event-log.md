---
id: "E122"
title: CLAP diagnostics, halt banner, and event log surfacing
created: 2026-04-26
tickets: ["0722", "0723", "0724", "0725"]
adrs: ["0044", "0051"]
---

## Goal

Surface compile diagnostics, engine halt state, and the rolling
event log in the CLAP webview, matching the TUI's behaviour. Route
observer-side diagnostics (`NotYetImplemented`, `InvalidSlot`) into
the same log so unsupported tap declarations don't silently no-op.

## Scope

1. Event log pane: bind to `GuiState::status_log`, newest-last,
   formatting mirrors `patches-player/src/tui.rs::LogEntry`.
2. Diagnostics panel: render `RenderedDiagnostic` list with severity
   colour, `file:line:col`, and label. Source-map projection stays in
   `patches-plugin-common`.
3. Halt banner: pinned top of the window when `GuiState::halt` is
   `Some(_)`. Formatting mirrors the TUI splash. Clears when the
   audio thread reports no halt.
4. Observer diagnostic routing: `SubscribersHandle::Diagnostic` items
   forwarded to `status_log` using the same `format_diagnostic`
   shape as the TUI.

## Out of scope

- File / module-path management (E123)
- Polish (E124)

## Acceptance

- Compile errors surface in the Diagnostics tab with location +
  label.
- Module panic produces a halt banner that clears on successful
  reload.
- Tap declarations that hit an unimplemented component appear in the
  event log with the standard "tap `X`: not yet implemented" line.
- `cargo clippy` and `cargo test` pass.
