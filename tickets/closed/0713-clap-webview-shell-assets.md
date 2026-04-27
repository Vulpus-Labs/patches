---
id: "0713"
title: New CLAP webview shell assets (HTML / JS / CSS)
priority: high
created: 2026-04-26
epic: "E120"
---

## Summary

Land the replacement shell for the CLAP webview: `index.html`,
`app.js`, `app.css`. Vanilla JS, no framework. Tab strip with three
tabs (Patch, Modules, Diagnostics), empty panes, and stub hooks for
`window.__patches.applyState` and `window.__patches.applyTaps`.

## Acceptance criteria

- [ ] `patches-clap/assets/index.html` exists; loaded via
      `WebViewBuilder::with_html(include_str!(...))`.
- [ ] `app.js` defines `window.__patches.applyState(snapshot)` and
      `window.__patches.applyTaps(frame)` as no-op stubs that store
      the latest payload on `window.__patches` for inspection.
- [ ] `app.css` provides a baseline tab-strip layout; tab clicks swap
      visible pane.
- [ ] Each tab visibly highlights when active.
- [ ] No external font / CDN dependency — assets are inlined or
      bundled.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

Keep CSS minimal here; full theme polish is E124.
