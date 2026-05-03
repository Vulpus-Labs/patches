---
id: "E124"
title: CLAP webview polish — resize, DPI, theme, DAW test pass
created: 2026-04-26
tickets: ["0730", "0731", "0732", "0733"]
adrs: []
---

## Goal

Final polish on the rebuilt CLAP webview: correct resize behaviour,
DPI scale handling, dark theme + focus styling, and a manual DAW
test pass on macOS and Windows.

## Scope

1. Resize: CLAP `gui.set_size` → `WebView::set_bounds`. CSS layout
   reflows.
2. DPI scale: respect the `scale` arg from `clap_window` in
   `patches-clap/src/gui.rs::create_gui`.
3. Dark theme CSS + keyboard focus rings on every interactive
   element.
4. Manual DAW test pass: Bitwig + Reaper on macOS, Reaper on
   Windows. Load patch, reload, scan dirs, halt-recover, resize,
   close / reopen — verify no leaks.

## Acceptance

- Resizing the plugin window in the host reflows the UI without
  visual artefacts.
- HiDPI displays render crisply.
- Test pass notes filed against this epic.
- `cargo clippy` and `cargo test` pass.
