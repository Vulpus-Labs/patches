---
id: "0675"
title: Drum modules — spectral and envelope assertions
priority: high
created: 2026-04-24
epic: E116
---

## Summary

`cymbal`, `tom`, `hihat` (closed + open), and similar percussion
modules currently assert only `rms > small_threshold` over a fixed
tick window after a trigger. These tests would pass on DC offset,
wrong pitch, constant noise, or a broken envelope. Add spectral
content and envelope-shape checks so real DSP drift fails the suite.

## Acceptance criteria

- [ ] `cymbal.rs` trigger test asserts HF spectral energy dominates LF
      (expected band ratio) and envelope peak occurs before sustained
      decay window.
- [ ] `tom.rs` trigger test asserts fundamental bin matches configured
      pitch within a documented tolerance, plus envelope decay
      monotonic after initial attack.
- [ ] `hihat.rs` (closed and open) asserts HF-dominant spectrum and
      that open-hihat decay window exceeds closed-hihat decay window
      by a sane ratio.
- [ ] Shared spectral helper lives alongside existing module test
      utilities — no duplication across files.

## Notes

Examples flagged by audit:
- `patches-modules/src/cymbal/tests.rs:188`
- `patches-modules/src/tom/tests.rs:175`
- `patches-modules/src/hihat/tests.rs:306, 357`

`patches-dsp` already has FFT helpers used by `vdco` tests — reuse
those where possible. See `patches-dsp` test support for
`fft_magnitudes` usage patterns.
