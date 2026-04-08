# E047 — patches-dsp test coverage gaps

## Goal

Close the coverage gaps and tighten the quality thresholds identified in the
patches-dsp test report (`patches-dsp/docs/test-report.md`). The crate has 208
passing tests with strong coverage of the mono filter and FFT paths, but several
components are untested or only tested at the API-mechanics level without
spectral or numerical verification.

## Background

A systematic review of the test report identified the following gaps, ordered by
priority:

1. **SlotDeck internals** — well-tested at the integration level but no unit
   tests for pool exhaustion recovery, processor-thread latency, or
   OverlapBuffer/ProcessorHandle edge cases beyond the two existing robustness
   tests.
2. **AtomicF32** — zero tests. Trivial wrapper but should have round-trip and
   ordering coverage.
3. **ADSR envelope shape** — ramp linearity is tested but the overall envelope
   shape (peak=1, sustain level, release-to-zero) is never checked for a given
   parameter set.
4. **PolyBiquad spectral coverage** — only 4 tests checking coefficient
   mechanics. No frequency response, SNR, or stability tests for the 16-voice
   path.
5. **Tighten numeric thresholds** — several thresholds have 2-5 orders of
   magnitude of headroom (e.g. biquad SNR 122 dB vs 60 dB threshold, convolution
   error 1e-6 vs 0.05). Tightening them would catch regressions earlier.
6. **PolyPhaseAccumulator / PolySvfKernel** — no dedicated tests; coverage is
   indirect via mono tests or a single parity check.
7. **SpectralPitchShifter end-to-end** — tested only on synthetic FFT bins, not
   on actual audio through the full FFT -> shift -> IFFT pipeline.
8. **Partitioned convolution exact latency** — tests check "output appears" but
   not that it appears at exactly the correct sample offset.

## Tickets

| # | Title | Priority |
|---|-------|----------|
| 0254 | SlotDeck pool exhaustion recovery and edge cases | medium |
| 0255 | AtomicF32 round-trip and ordering tests | low |
| 0256 | ADSR end-to-end envelope shape test | medium |
| 0257 | PolyBiquad spectral and stability coverage | medium |
| 0258 | Tighten patches-dsp numeric test thresholds | low |
| 0259 | PolySvfKernel and PolyPhaseAccumulator dedicated tests | low |
| 0260 | SpectralPitchShifter end-to-end audio test | low |
| 0261 | Partitioned convolution exact latency assertions | low |

## Non-goals

- Rewriting existing passing tests. The goal is to add missing coverage and
  tighten thresholds, not restructure what already works.
- Coverage metrics or line-level coverage tooling. The gaps were identified by
  reviewing test intent against component behaviour, not by running tarpaulin.
