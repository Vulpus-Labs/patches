---
id: "0544"
title: Close spectral_pitch_shift behavioural test gaps
priority: low
created: 2026-04-17
epic: E092
---

## Summary

[patches-dsp/src/spectral_pitch_shift/tests.rs](../../patches-dsp/src/spectral_pitch_shift/tests.rs)
has 11 `#[test]` functions covering identity at ratio 1.0, octave-up
bin motion, principal-argument wrap, and linear interpolation. Three
named gaps remain. Add them.

## Acceptance criteria

- [ ] **Grain-boundary continuity.** A test feeds a stationary sine
      (e.g. 440 Hz at 48 kHz) through at least two hops at
      ratio 1.2. Reconstructs the time-domain output and asserts
      that the per-frame difference stays bounded across the hop
      boundary — i.e. no phase-flip spike when grain N+1 takes over
      from grain N. Exact bound is test-author's call; a
      reasonable shape is `max(abs(diff)) <= 0.05` on the
      reconstructed output.
- [ ] **Formant preservation.** A test with `preserve_formants =
      true` feeds a synthetic input with a known spectral envelope
      (e.g. an impulse shaped by a single biquad peaking filter)
      at a non-unity ratio and asserts that the envelope peak's
      frequency-bin location in the output is within N bins of
      the input's peak location, where N is tight (1 or 2). A
      companion test without formant preservation shows the peak
      shifts with the ratio.
- [ ] **Mono / poly parity.** A test that runs the mono path and
      the poly (per-bin) path on the same stationary tone and
      asserts the two outputs agree to within a defined tolerance
      after one grain of settling. Documents precisely what
      "agree" means — the two paths are not required to be bit-
      identical, but on a pure tone they should produce the same
      spectral peak in the same bin.
- [ ] Tests land in the appropriate category file if
      `spectral_pitch_shift/tests.rs` has been split by the time
      this ticket runs; otherwise in the current `tests.rs`.
- [ ] `cargo test -p patches-dsp` clean.

## Notes

The three tests document three distinct failure modes the existing
coverage will not catch:

1. **Grain boundary.** A bug in overlap-add window weighting or
   hop alignment produces an audible click at grain boundaries;
   no current test will see it.
2. **Formant preservation.** The `preserve_formants` flag is
   exposed to DSL users but no test currently runs that branch.
3. **Mono/poly parity.** The two modes use different code paths
   (region vs. per-bin) and can drift independently.

If any of these tests is hard to write because the internal API
isn't reachable, that's a signal to add the minimum pub surface
(rather than skipping the test). Document any such addition in the
PR.

No behavioural change. Pure test addition.
