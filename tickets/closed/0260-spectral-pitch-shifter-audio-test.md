---
id: "0260"
title: SpectralPitchShifter end-to-end audio test
priority: low
created: 2026-04-02
---

## Summary

`SpectralPitchShifter` is tested only on synthetic pre-built FFT spectra (unit
tests in `spectral_pitch_shift.rs`). No test runs it on actual audio through the
full pipeline: generate signal -> window -> FFT -> `transform()` -> IFFT ->
overlap-add -> verify output pitch. The SlotDeck integration tests
(`pitch_shift_octave_up_doubles_frequency` etc.) cover a similar path but go
through the full SlotDeck threading machinery, making failures harder to
diagnose.

## Acceptance criteria

- [ ] Test that a 440Hz sine processed through FFT -> SpectralPitchShifter(+12
      semitones) -> IFFT -> OLA produces output whose dominant frequency is 880Hz
      (within +/-1 bin of a 1024-point FFT at 48kHz).
- [ ] Test that identity shift (0 semitones) preserves the signal with error RMS
      < 10% of signal RMS.
- [ ] Test that the mix parameter at 0.0 returns the original signal (within
      1e-4 RMS error).
- [ ] Tests use `RealPackedFft` directly (not SlotDeck) to isolate the pitch
      shifter from the threading/buffering layer.
- [ ] Tests in `patches-dsp/tests/` as integration tests, or in
      `spectral_pitch_shift.rs` if the FFT is accessible.

## Notes

These tests complement rather than replace the SlotDeck pitch-shift tests. The
goal is a simpler, more diagnosable test path that exercises the pitch shifter's
spectral processing without the SlotDeck buffering machinery.
