---
id: "0708"
title: Horizontal meters in patches-player TUI
priority: medium
created: 2026-04-26
---

## Summary

Vertical bar layout (0704) does not leave enough room for tap-name
labels at typical pane widths. Rotate to horizontal: one row per
tap, name on the left, peak+RMS bars filling the remaining width,
dB value on the right.

## Acceptance criteria

- [ ] One row per declared meter tap. Layout: `name | bar | dB`.
- [ ] Two stacked sub-rows per tap (peak above RMS) or a single
  composite bar with peak as a thin trailing tick — pick whichever
  reads cleaner at 30 Hz refresh; document the choice.
- [ ] Name column truncates at fixed width (e.g. 16 chars) with
  ellipsis. Bar fills `pane_width - name_w - db_w - margins`.
- [ ] dB readout right-aligned, fixed width (e.g. `-60.0 dB`).
- [ ] Colour thresholds unchanged (green/amber/red at -18 / -6 dB).
- [ ] Vertical scroll if taps exceed pane height (replaces
  horizontal scroll).
- [ ] Drop indicator: small `d{n}` suffix on the row, magenta.
- [ ] Tests: visible-rows calculation, name truncation, scroll
  clamp on `set_taps`.
- [ ] Manual verification: load a patch with 8+ meter taps with
  varied name lengths; confirm readability at terminal widths
  80, 120, 200.

## Notes

- Bar character cell granularity: 1 cell = `1 / pane_width`
  fraction of dynamic range. At 80 columns minus overhead this is
  ~0.5 dB per cell at the top of the scale, fine for visual
  feedback.
- Consider Unicode block elements (`▏▎▍▌▋▊▉█`) for sub-cell
  precision on the bar trailing edge.
