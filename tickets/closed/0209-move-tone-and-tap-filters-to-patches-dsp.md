---
id: "0209"
title: Move tone_filter and tap_feedback_filter to patches-dsp and add tests
priority: medium
created: 2026-03-30
---

## Summary

`patches-modules/src/common/tone_filter.rs` (one-pole shelving filter) and
`patches-modules/src/common/tap_feedback_filter.rs` (one-pole filter + drive
saturation for delay feedback paths) are pure-algorithm types with no module-harness
dependencies. Moving them to `patches-dsp` and adding missing frequency-response and
stability tests completes their coverage per ADR 0022.

## Acceptance criteria

- [ ] Both files moved to `patches-dsp`; re-exported from crate root or a named
      module.
- [ ] `patches-modules` imports from `patches-dsp`; no duplication.
- [ ] All existing tests migrate and pass (5 tests total).
- [ ] **T2 — ToneFilter frequency response:** Assert output amplitude at three spot
      frequencies (e.g. 100 Hz, 1 kHz, 10 kHz at 48 kHz sample rate) for both
      `tone = 0.0` and `tone = 1.0` settings against the expected shelving response
      (bright/dark).
- [ ] **T2 — TapFeedbackFilter frequency response:** Assert passband gain ≈ 1.0 at
      a low frequency; confirm the filter shapes (not passes flat) at higher
      frequencies.
- [ ] **T4 — TapFeedbackFilter stability:** Drive with maximum-amplitude input and
      maximum drive for 10,000 samples; assert output remains bounded (no
      NaN/infinity).
- [ ] **T7 — State reset:** Assert that processing the same input sequence twice with
      state reset between runs produces bit-identical output for both filter types.
- [ ] `cargo test` (workspace) passes; `cargo clippy` clean.

## Notes

Technique references (ADR 0022): T2, T4, T7.
