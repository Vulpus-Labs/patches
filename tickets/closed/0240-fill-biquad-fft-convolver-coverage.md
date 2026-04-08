---
id: "0240"
title: Fill biquad, FFT, and partitioned convolver test coverage gaps
priority: high
created: 2026-04-01
---

## Summary

Three complex DSP algorithms have thin or missing test coverage for their
primary behavioural properties. This ticket adds frequency-response, round-trip,
and integration tests to bring them up to the standard set by SVF, halfband,
and tone_filter.

## Acceptance criteria

### biquad.rs

- [ ] At least 3 frequency-response tests using `magnitude_response_db()` from
      `test_support`: lowpass passes below cutoff, attenuates above; highpass
      inverse; bandpass peaks at centre.
- [ ] Test that the `saturate` flag clips output (currently untested code path).
- [ ] Test coefficient stability for extreme cutoff/Q values (very low, very
      high, near Nyquist).

### fft.rs

- [ ] Forward/inverse round-trip test: `forward()` then `inverse()` recovers
      the original signal within floating-point tolerance.
- [ ] Real-signal symmetry: forward transform of a real signal has conjugate-
      symmetric bins (verify packed layout obeys this).
- [ ] Parseval's theorem: energy in time domain equals energy in frequency
      domain (within tolerance).

### partitioned_convolution.rs

- [ ] End-to-end impulse-response test: convolve with a known IR, verify output
      matches direct (non-partitioned) convolution within tolerance.
- [ ] Multi-partition test: use an IR longer than one partition and verify
      correct overlap-save stitching.
- [ ] Latency test: verify that the first non-zero output appears at the
      expected sample offset.

## Notes

Use `assert_passband_flat!`, `assert_stopband_below!`, and
`magnitude_response_db()` from `patches-dsp/src/test_support.rs` where
applicable. The biquad tests should follow the pattern established in
`svf.rs` and `halfband.rs`.
