---
id: "E121"
title: CLAP tap rendering parity — meter / scope / spectrum widgets
created: 2026-04-26
tickets: ["0716", "0717", "0718", "0719", "0720", "0721"]
adrs: ["0053", "0055"]
---

## Goal

Bring live tap data into the CLAP webview, matching the rendering
behaviour of the ratatui `patches-player` TUI (see
`patches-player/src/tui.rs`). Each declared tap renders as a meter,
scope, or spectrum widget on the Patch tab, driven by a dedicated
`applyTaps` IPC channel that bypasses the snapshot dedupe throttle.

## Scope

1. New `TapFrame` JSON channel in `patches-plugin-common`:
   per-slot peak / RMS / scope samples / spectrum bins. JS hook
   `window.__patches.applyTaps(frame)`.
2. Plugin-side pump: hand `SubscribersHandle` to the webview shell;
   `on_main_thread` reads subscribers each tick, serialises a
   `TapFrame`, calls `evaluate_script` at ~30 Hz with cheap dedupe.
3. JS meter widget (Canvas2D): vertical + horizontal bars, dB scale,
   colour thresholds matching TUI constants (`DB_AMBER_FLOOR = -18`,
   `DB_RED_FLOOR = -6`, `DB_FLOOR = -60`).
4. JS scope widget: line plot of `SCOPE_BUFFER_LEN` samples.
5. JS spectrum widget: bar plot of `SPECTRUM_BIN_COUNT` bins, log-x,
   dB-y.
6. Manifest-driven layout: one widget per `TapDescriptor`, ordered by
   slot, widget chosen by tap kind. Compound taps render their
   component group as a unit.

## Out of scope

- Diagnostics, halt banner, event log (E122)
- File / module-path management (E123)
- Polish (E124)

## Acceptance

- Loading a patch with declared taps renders the corresponding
  widgets on the Patch tab, updating in real time.
- Audio behaviour, dB thresholds, scope buffer length, and spectrum
  bin layout match the TUI byte-for-byte where comparable.
- No JS-side polling — frames are pushed by `on_main_thread`.
- `cargo clippy` and `cargo test` pass.
