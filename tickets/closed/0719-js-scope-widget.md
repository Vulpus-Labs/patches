---
id: "0719"
title: JS scope widget (line plot of SCOPE_BUFFER_LEN samples)
priority: medium
created: 2026-04-26
epic: "E121"
---

## Summary

Line-plot oscilloscope widget for `osc` taps. Renders
`SCOPE_BUFFER_LEN` samples per frame, time on X, amplitude on Y.

## Acceptance criteria

- [ ] Widget class in `app.js` rendering a Canvas2D line plot.
- [ ] Axis range: full buffer width, amplitude clamped to ±1.0 with
      visible 0-line.
- [ ] Updates each frame from `applyTaps`.
- [ ] Visual matches the ratatui scope canvas in
      `patches-player/src/tui.rs`.
- [ ] `cargo clippy` and `cargo test` clean.
