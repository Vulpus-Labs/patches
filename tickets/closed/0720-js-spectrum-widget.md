---
id: "0720"
title: JS spectrum widget (bar plot of SPECTRUM_BIN_COUNT bins)
priority: medium
created: 2026-04-26
epic: "E121"
---

## Summary

Spectrum widget for `spectrum` taps. Bar plot of
`SPECTRUM_BIN_COUNT` bins, log-X frequency, dB-Y magnitude.

## Acceptance criteria

- [ ] Widget class in `app.js` rendering a Canvas2D bar plot.
- [ ] Log-frequency X axis using `SPECTRUM_FFT_SIZE` to derive bin
      centre frequencies.
- [ ] dB-magnitude Y axis with floor matching meter `DB_FLOOR`.
- [ ] Updates each frame from `applyTaps`.
- [ ] Visual matches the ratatui spectrum view in
      `patches-player/src/tui.rs`.
- [ ] `cargo clippy` and `cargo test` clean.
