# E045 — FFT test helpers and spectral assertions

## Goal

Introduce shared FFT-based test utilities in `patches-dsp` and use them to
strengthen existing tests. After this epic, `patches-dsp` has a small
frequency-domain test toolkit (`test_support.rs`) and several test suites
express their assertions in spectral terms — passband flatness, stopband
rejection, spectral slope, dominant bin — rather than ad-hoc time-domain
proxies.

## Background

`RealPackedFft` already exists in `patches-dsp` but most filter and noise tests
measure frequency response by driving sinusoids one frequency at a time and
computing RMS or peak amplitude after a warmup period. This is indirect, slow
(one filter run per frequency), and sensitive to warmup-length choices. An
impulse-response FFT gives the full transfer function in one pass.

Noise tests currently use time-domain variance proxies (boxcar averaging,
first-differences) instead of directly measuring the power spectral density
and asserting the expected slope.

The `dominant_bin` helper is duplicated across `spectral_pitch_shift.rs` and
`tests/slot_deck.rs`.

## Tickets

| #      | Title                                                       | Priority |
| ------ | ----------------------------------------------------------- | -------- |
| T-0234 | Spectral test helpers and assertion macros                   | high     |
| T-0235 | Noise spectral shape tests via FFT                           | medium   |
| T-0236 | SVF frequency response via impulse-response FFT              | medium   |
| T-0237 | ToneFilter frequency response via impulse-response FFT       | medium   |
| T-0238 | HalfbandFir full transfer function test                      | low      |
| T-0239 | THD helper and approximation distortion tests                | low      |
