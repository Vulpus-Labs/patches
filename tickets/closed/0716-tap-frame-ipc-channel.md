---
id: "0716"
title: TapFrame IPC channel (plugin-common + JS hook)
priority: high
created: 2026-04-26
epic: "E121"
---

## Summary

Define the `TapFrame` JSON contract used to push live tap data from
the plugin into the webview, separate from `GuiSnapshot`. Lives in
`patches-plugin-common`. JS side gains
`window.__patches.applyTaps(frame)`.

## Acceptance criteria

- [ ] `TapFrame` struct in `patches-plugin-common::gui` (or sibling
      module). Fields: `v: u32` version, plus per-slot data: peak,
      RMS, optional scope sample buffer, optional spectrum bin array.
- [ ] Serialises to a compact JSON shape suitable for ~30 Hz push.
- [ ] Round-trip serde tests cover meter-only, scope, spectrum, and
      compound frames.
- [ ] `app.js` `applyTaps` stub stores the latest frame on
      `window.__patches.lastFrame` for inspection.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

Keep field names short — every byte is pushed at frame rate.
