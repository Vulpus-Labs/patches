---
id: "0752"
title: CLAP webview state push race — JS not ready when first applyState fires
priority: high
created: 2026-04-28
---

## Summary

After the CLAP host closes and reopens the plugin window, the webview UI is
blank: no loaded module path, no meter/scope displays, no module-path list,
no diagnostic history. The audio engine keeps running and `GuiState`
survives `gui_destroy` / `gui_create` (it lives on the plugin, not the GUI
handle), so the data is there — but the UI never receives it.

Root cause is in `WebviewGuiHandle::update` and `push_taps`
(`patches-clap/src/gui.rs`). On first push after `gui_create` the webview
exists but the page's JS bundle may not yet have parsed and defined
`window.__patches`. The injected script is

```js
window.__patches && window.__patches.applyState(...)
```

which short-circuits to a no-op. `webview.evaluate_script` returns `Ok`
regardless (it only reports script submission, not JS runtime result), so
the handler caches the JSON in `last_snapshot` / `last_tap_json` and the
subsequent identical snapshots dedupe — the UI is never repopulated.

## Acceptance criteria

- [ ] After `gui_destroy` followed by `gui_create`, the UI repopulates
      with the current `GuiState` (file path, module paths, diagnostics,
      taps) within one update tick of the JS bundle finishing load.
- [ ] No reliance on a fixed delay or "skip first N pushes" heuristic.
- [ ] No regression in dedupe behaviour during steady-state operation.

## Notes

Suggested approach: JS posts an IPC message `{"kind":"ready"}` once
`window.__patches` is wired up. The Rust IPC handler clears
`last_snapshot` and `last_tap_json` on receipt; next `update` /
`push_taps` will fall through the dedupe and push the current state.

Same race affects `push_taps` (`gui.rs:257`) — fix both caches together.
