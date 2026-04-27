---
id: "E120"
title: CLAP webview shell rebuild — wipe spike, reshape IPC contract
created: 2026-04-26
tickets: ["0712", "0713", "0714", "0715"]
adrs: []
---

## Goal

Throw out the quick-and-dirty webview spike and rebuild the CLAP
plugin's webview shell on a clean foundation. No feature parity yet —
this epic just lands the new HTML/JS/CSS scaffold, reshapes the
`GuiSnapshot` / `Intent` JSON contract for the tap-driven world, and
keeps the existing wry parented webview + throttle/dedupe push
machinery. Plugin loads, blank UI renders, all `Intent` round-trips.

## Scope

1. Delete spike: `assets/hello.html`, `applyMeter` channel,
   `meter_poll_requested` poll model, dead `GuiSnapshot` fields.
2. New shell assets: `assets/index.html`, `assets/app.js`,
   `assets/app.css`. Tab strip (Patch / Modules / Diagnostics), empty
   panes, vanilla JS, no framework.
3. Reshape `GuiSnapshot` in `patches-plugin-common`: tap manifest
   projection (name / slot / kind / components), bump `VERSION`. Keep
   throttle + dedupe in `patches-clap/src/gui.rs`.
4. Confirm `Intent` set (Browse, Reload, Rescan, AddPath, RemovePath),
   drop `PollMeter`, wire button stubs that POST each intent shape.

## Out of scope

- Live tap data (E121)
- Diagnostics / halt / event log rendering (E122)
- File header + module-paths tab functionality (E123)
- Resize / DPI / theme polish (E124)

## Acceptance

- Plugin loads in Bitwig / Reaper on macOS; blank tabbed UI renders.
- Each tab-strip / button click posts the correct `Intent` JSON;
  `on_main_thread` drains the corresponding flag.
- No references to `applyMeter`, `meter_poll_requested`, or
  `hello.html` remain in the workspace.
- `cargo clippy` and `cargo test` pass.
