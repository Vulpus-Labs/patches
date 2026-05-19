---
id: "0938"
title: Interleave FDN delay lines into one row-packed buffer
priority: medium
created: 2026-05-19
---

## Summary

After 0936 (SIMD biquad) and 0937 (SoA LFO), per-line profiling showed
the per-sample delay-line writes still costing ~4.7 ns — eight scattered
single-`f32` stores into eight separate ring buffers. Packing the eight
lines into one `[[f32; 8]; capacity]` buffer collapses the writes into a
single 32-byte (half cache line) contiguous store.

Reads stay per-voice scalar with linear interpolation: each line reads
at a different LFO-modulated offset, so SoA gives no payoff on the read
side. The interleaved layout costs nothing on misses (same offsets =
same number of cache lines fetched) and turns out to help slightly on
hits — adjacent rows are now prefetcher-friendly.

## Acceptance criteria

- [x] `FdnReverbKernel` stores delays in one `InterleavedDelays` (inline
      type) instead of `[DelayBuffer; LINES]`.
- [x] Per-sample write is one row push (`[f32; 8]`).
- [x] Per-sample reads use a new `read_one_linear(voice, offset)` that
      pulls one voice with the same linear-interp semantics as
      `DelayBuffer::read_linear`. No change to interpolation accuracy.
- [x] FDN tests pass (impulse decay, DC bounded, stereo decorrelation,
      early reflections, bounded energy across all characters).
- [x] Kernel-direct bench shows measurable mean-ns/tick drop, 5-run
      stable. Tail behaviour (p99.9, max) tightens as a bonus.

## Outcome

Kernel-direct bench, 5 runs of 10M samples at 48 kHz:

| variant                        | mean ns | p50 | p99.9 | max   |
| ------------------------------ | ------- | --- | ----- | ----- |
| original (pre-0936)            | 50.5    | 50  | ~400  | 39000 |
| + SIMD biquad (0936)           | 40.0    | 38  | ~250  | 12000 |
| + SoA LFO (0937)               | 36.5    | 36  | ~250  | ~500  |
| + interleaved delays (this)    | 33.5    | 34  | 57    | 266   |

**~8% additional speedup. Cumulative kernel: 50 → 33.5 ns ≈ 33%.**

Tail behaviour is the surprise: p99.9 dropped from ~250 ns to 57 ns and
max from thousands to ~266. One contiguous store per tick beats eight
scattered stores for predictability, not just throughput.

Samply confirms the cost moved:

| line                               | pre  | post | Δ    |
| ---------------------------------- | ---- | ---- | ---- |
| delay write (now row push)         | 4.7  | 0.9  | −3.8 |
| delay read (now `read_one_linear`) | 7.7  | 6.2  | −1.5 |

The remaining heaviest line is still the per-voice delay read at ~6.2 ns;
that one is genuinely scalar (8 independent buffer addresses) and isn't
amenable to further SIMD without an accuracy compromise like the
2×-upsample-at-write scheme (option A from the discussion).

## Notes

- `InterleavedDelays` is kept inline in the kernel module; the FDN is the
  only consumer with this particular read pattern (different offset per
  voice, single-voice extraction). `PolyDelayBuffer` in patches-dsp
  serves the different "all voices at one offset" pattern and wasn't a
  good fit.
- `cap_max` derivation changes from `self.delays[0].capacity()` to
  `self.delays.capacity()` — single shared capacity now.
