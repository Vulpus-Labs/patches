# E039 — ADR-0022 Phase 2: High-priority migration tasks

## Goal

Address the P1 and P2 items identified in the E038 audit (T-0204). The primary
outcomes are:

- `HalfbandFir` gains real automated assertions (currently zero).
- `fast_tanh` gains test coverage (currently zero).
- The biquad kernel moves to `patches-dsp` with independent transfer-function
  tests, decoupling 25 existing module-level filter tests from DSP correctness.
- Four `patches-modules/common/` pure-algorithm files move to `patches-dsp`
  where they belong.
- The SVF kernel is extracted to a standalone type in `patches-dsp` with
  proper tests.
- `HalfbandInterpolator` gains a missing stopband attenuation assertion.

## Background

See `docs/src/technical/dsp-test-audit.md` and ADR 0022
(`adr/0022-externalisation-of-dsp-logic.md`).

## Suggested ordering

The tickets are independent unless noted. Suggested order:

1. T-0205 and T-0206 first — pure test additions, no code moves, lowest risk.
2. T-0211 — small assertion addition to an existing test file.
3. T-0208 — mechanical moves with no new API design required.
4. T-0209 — mechanical moves with some new tests.
5. T-0207 — biquad move (highest impact; gives patches-dsp its first filter kernel).
6. T-0210 — SVF extraction (depends on biquad patterns established in T-0207).

## Tickets

| # | Title | Priority |
| --- | --- | --- |
| T-0205 | Add real assertions to HalfbandFir tests | P1 |
| T-0206 | Add tests for fast_tanh | P1 |
| T-0207 | Move biquad kernel to patches-dsp and add independent tests | P1 |
| T-0208 | Move approximate.rs and waveforms.rs to patches-dsp | P2 |
| T-0209 | Move tone_filter and tap_feedback_filter to patches-dsp and add tests | P2 |
| T-0210 | Extract SVF kernel to patches-dsp and add tests | P2 |
| T-0211 | Add stopband attenuation assertion to HalfbandInterpolator tests | P2 |
