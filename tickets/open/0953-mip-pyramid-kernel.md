---
id: "0953"
title: Mip pyramid kernel (patches-dsp)
priority: medium
created: 2026-05-22
epic: E155
---

## Summary

Add a kernel to `patches-dsp` that builds an octave-spaced, bandlimited **mip pyramid**
from a sample buffer and reads it back at a variable rate with anti-aliasing. Same idea
as texture mipmapping: when the player pitches a sample *up* (reads the table faster),
naïve resampling folds high frequencies back as alias. Pre-filtered octave copies let
the reader pick a level whose Nyquist already covers the playback rate.

Pyramid construction is **load-time / non-RT** (allocates once, off the audio thread);
the read path is alloc-free and RT-safe.

## Acceptance criteria

- [ ] New `patches-dsp/src/mip_pyramid.rs`, re-exported from the crate root.
- [ ] `MipPyramid::from_samples(&[f32], ...) -> MipPyramid`: builds octave levels by
      repeatedly **decimating-by-2 with the existing `HalfbandFir`**
      (`patches-dsp/src/halfband.rs`, `process(first, second) -> f32`) down to a small floor
      (e.g. ≥ 64 samples). Owns its `Vec<Vec<f32>>` (or packed) storage — non-RT.
- [ ] A read method taking a fractional read position and a playback ratio `r`, returning
      one sample: select mip level `≈ log2(r)`, linear-interpolate within the table (the
      mip already bandlimits, so linear interp suffices for v1).
- [ ] **v1 = nearest mip level** (no inter-level blend); leave a documented hook for
      trilinear-style blending of `floor`/`ceil` levels later.
- [ ] No RT allocation in the read path; construction may allocate. No `unwrap`/`expect`.
- [ ] Tests: a pyramid of a known sine has ~halved length per level; reading at `r = 2`
      selects a lower level and shows materially less aliasing energy than reading the base
      table at `r = 2` (FFT or simple high-band energy check); read position clamps/loops
      sanely at bounds.

## Notes

- Reuse don't reinvent: `HalfbandFir` is the decimator; `MonoPhaseAccumulator`
  (`patches-modules/src/common/phase_accumulator.rs`) is the read-position generator on the
  module side (0954) — this kernel just needs position + ratio in.
- Storage ≈ 2× the original (geometric series) — cheap; note it in the doc-comment.
- Decide loop handling now or defer to 0954: a looping reader needs loop-point wrapping in
  the read position; the pyramid itself is loop-agnostic.
- Validation: `just inner -p patches-dsp`.
