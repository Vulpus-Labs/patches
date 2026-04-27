---
id: "0730"
title: Webview resize — wire CLAP gui.set_size to WebView::set_bounds
priority: medium
created: 2026-04-26
epic: "E124"
---

## Summary

Honour CLAP `gui.set_size` by updating the wry `WebView`'s bounds.
CSS layout reflows automatically.

## Acceptance criteria

- [ ] `extensions.rs` `gui.set_size` callback updates the
      `WebviewGuiHandle`'s bounds.
- [ ] Resizing the plugin window in Bitwig / Reaper reflows the UI
      smoothly with no clipping or scrollbar artefacts.
- [ ] `gui.get_size`, `gui.adjust_size`, and `gui.can_resize` agree
      on the supported size range.
- [ ] `cargo clippy` and `cargo test` clean.
