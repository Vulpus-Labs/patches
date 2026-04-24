---
id: "0671"
title: Webview ↔ Rust IPC for GuiState sync
priority: high
created: 2026-04-24
---

## Summary

Bidirectional sync between `GuiState` (Rust main thread) and the
webview's JS runtime. JS can observe state changes and emit intents
(browse, reload, rescan, add path, remove path). Rust pushes state
diffs at a bounded cadence; JS pushes intent messages via wry's
`ipc_handler`.

## Acceptance criteria

- [ ] JS → Rust: `ipc_handler` parses JSON messages with a tagged union
      of intents matching the `*_requested` fields in `GuiState`.
      Handler flips the corresponding flags under the existing
      `Arc<Mutex<GuiState>>`.
- [ ] Rust → JS: main-thread tick (driven by CLAP `on_main_thread` or a
      host timer) serialises current `GuiState` snapshot and calls
      `webview.evaluate_script("window.__patches.applyState(...)")`.
      Cadence capped at ~30 Hz; skip if state unchanged.
- [ ] Snapshot format versioned with a `v` field.
- [ ] No audio-thread involvement; all IPC is main-thread.
- [ ] JS global `window.__patches` exposes `applyState(snapshot)` and
      `send(intent)` primitives; documented in a comment in the HTML
      shell.
- [ ] Integration smoke: JS `send({kind:"reload"})` triggers the same
      reload path as the vizia Reload button.

## Notes

`GuiState` is plain data — derive / hand-write `serde::Serialize` for
the snapshot. `SourceMap` and `RenderedDiagnostic` may need projection
to a simpler shape; keep that in the webview crate, not common.

Meter/tap data is *not* part of this snapshot — that path is separate
and optimised for higher rates (ticket 0673).
