---
id: "0754"
title: Stabilise oscilloscope display via period-aligned window
priority: low
created: 2026-04-29
---

## Summary

The scope's zero-cross snap phase-locks the left edge of the display
but the right edge drifts whenever the waveform period does not divide
the window length, producing visible "breathing" at non-integer
refresh-rate / fundamental ratios. Period-align the window so both
edges sit on rising zero-crossings: the waveform appears stationary
regardless of frame rate.

Pure client-side change in `patches-clap/assets/app.js`. Processor
emits raw samples already (display transforms are client-side per the
comment at `patches-observation/src/processor.rs:562`); no Rust, ABI,
or tap-data changes.

## Acceptance criteria

- [ ] `findLatestZeroCross` (or replacement) collects all rising
      crossings in the window, not just the latest.
- [ ] Period estimated from median of inter-crossing diffs; alignment
      skipped (fall back to current behaviour) if diffs are
      inconsistent (e.g. max/min ratio > 1.2) or fewer than two
      crossings exist.
- [ ] Display length truncated to `floor((n - k) / period) * period`
      and scaled to canvas width.
- [ ] Hysteresis on the trigger (`prev < -ε && curr >= +ε`) to avoid
      re-triggering on noise or harmonic ripple.
- [ ] Existing snap toggle still works; aperiodic / silent inputs
      degrade gracefully to the raw window.

## Notes

Sub-sample trigger interpolation considered and rejected — 1-sample
jitter is acceptable.

EMA-smoothed period across frames is a possible follow-up if amplitude
dips cause lock loss in practice; not required for first cut.

Parked behind other in-flight work (stereo port rename, CLAP webview
handshake). Pick up once those land to avoid churn in `app.js`.
