---
id: "0276"
title: Integrate baseview window with CLAP GUI extension lifecycle
priority: high
created: 2026-04-07
---

## Summary

Wire the vizia/baseview window into the CLAP GUI extension callbacks so that
the host can create, parent, show, hide, and destroy the plugin window. Replace
the macOS-specific code paths in `extensions.rs` with calls to the new
`ViziaGuiHandle`.

## Acceptance criteria

- [ ] `gui_set_parent()` creates a baseview window embedded in the
      host-provided parent (NSView on macOS, HWND on Windows, X11 Window on
      Linux).
- [ ] `gui_destroy()` tears down the baseview window cleanly.
- [ ] `gui_show()` and `gui_hide()` control baseview window visibility
      (replacing the current TODO stubs).
- [ ] `gui_set_scale()` forwards the DPI scale factor to the vizia context.
- [ ] `gui_get_size()` returns the actual vizia window size.
- [ ] Opening and closing the GUI multiple times does not leak resources or
      crash.
- [ ] The plugin loads and displays its GUI in at least one DAW on macOS (e.g.
      REAPER or Bitwig).
- [ ] `cargo clippy -p patches-clap` passes with no warnings.

## Notes

- The CLAP GUI extension API values for `CLAP_WINDOW_API_COCOA`,
  `CLAP_WINDOW_API_WIN32`, and `CLAP_WINDOW_API_X11` are already handled in
  `gui_is_api_supported`; the implementation needs to pass the correct parent
  handle variant to baseview.
- baseview's `WindowOpenOptions` takes a `raw_window_handle` parent — check
  whether vizia exposes this or if the baseview API must be called directly.
- Depends on T-0275.
