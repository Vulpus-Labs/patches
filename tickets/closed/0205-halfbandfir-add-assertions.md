---
id: "0205"
title: Add real assertions to HalfbandFir tests
priority: high
created: 2026-03-30
---

## Summary

The three existing tests in `patches-dsp/src/halfband.rs` (`print_impulse_response`,
`print_dc_response`, `print_nyquist_response`) are entirely `println!` output with
no assertions. `HalfbandFir` is a load-bearing DSP primitive used by the oversampling
pipeline; it currently has zero automated verification. This ticket replaces or
supplements those tests with real assertions.

## Acceptance criteria

- [ ] **T1 — Impulse response:** Feed a unit impulse and assert the output sequence
      matches the known tap coefficients (within f64 rounding; the filter is linear
      and time-invariant so the impulse response _is_ the tap vector).
- [ ] **T2 — Passband gain:** Drive with a sinusoid well below cutoff (e.g. fs/8)
      and assert steady-state output amplitude is within ±0.1 dB of unity.
- [ ] **T2 — Stopband attenuation:** Drive with a sinusoid in the stopband (e.g.
      fs × 0.4, well above the half-band) and assert amplitude is attenuated by at
      least 60 dB.
- [ ] **T3 — DC:** Drive with a DC signal (constant 1.0) and assert output converges
      to 1.0 within tolerance (after enough samples to flush the FIR delay).
- [ ] **T3 — Nyquist:** Drive with alternating ±1.0 (Nyquist) and assert output
      converges to ≈ 0.0 (halfband cutoff is fs/4; Nyquist should be in stopband).
- [ ] Existing print-based tests may be removed or converted; no print-only tests
      should remain.
- [ ] `cargo test -p patches-dsp` passes; `cargo clippy -p patches-dsp` clean.

## Notes

Technique references (ADR 0022): T1, T2, T3.

The tap coefficients for the 33-tap default filter are already visible in the
source; the impulse response test simply needs to assert `output[n] == taps[n]`
for the relevant positions in the output stream.

For the frequency-response tests: process enough samples to flush the group delay
(at least 33 samples), then measure steady-state amplitude.
