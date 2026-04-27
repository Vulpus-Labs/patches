---
id: "0718"
title: JS meter widget (Canvas2D, dB scale, colour thresholds)
priority: high
created: 2026-04-26
epic: "E121"
---

## Summary

Implement the meter widget in the webview: vertical and horizontal
bar variants, peak + RMS, dB scale, colour thresholds matching the
TUI constants.

## Acceptance criteria

- [ ] Widget class in `app.js` rendering a Canvas2D meter from a
      `{ peak, rms }` pair.
- [ ] Colour thresholds: green below `DB_AMBER_FLOOR = -18 dB`,
      amber `-18..-6 dB`, red above `DB_RED_FLOOR = -6 dB`.
- [ ] Floor at `DB_FLOOR = -60 dB`; below floor renders empty.
- [ ] Both vertical and horizontal orientations supported.
- [ ] Visual matches the ratatui implementation in
      `patches-player/src/tui.rs` for equivalent inputs.
- [ ] `cargo clippy` and `cargo test` clean.

## Notes

Constants must agree with the TUI — extract them once in
`patches-plugin-common` and serialise into the snapshot if it makes
sense, otherwise duplicate by reference.
