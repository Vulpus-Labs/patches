# E041 — ADR-0022 Phase 4: Remaining tasks and clean-up

## Goal

Complete the remaining migration and testing work identified in the Phase 3
review (E040). Close out all outstanding ADR 0022 compliance items.

After E041 the `patches-dsp` crate will contain all pure DSP algorithms with
independent, rigorous tests; `patches-modules` tests will verify protocol and
wiring only.

## Background

See `docs/src/technical/dsp-test-audit.md` (updated in E040) and ADR 0022
(`adr/0022-externalisation-of-dsp-logic.md`).

## Suggested ordering

1. T-0212 first — pure test additions, no code moves, lowest risk.
2. T-0216 — small test additions to existing `patches-dsp` files.
3. T-0214 — ADSR extraction (self-contained state machine, simple dependencies).
4. T-0215 — noise extraction (PRNG is simple; spectral test is the main effort).
5. T-0213 — oscillator extraction (most complex; touch multiple module files).
6. T-0217 and T-0218 — deferred P4 tests; can run in parallel with P3 work.

## Tickets

| # | Title | Priority |
| --- | --- | --- |
| T-0212 | Add T7 state-reset tests for patches-dsp stateful types | P3 |
| T-0213 | Extract oscillator phase accumulator and PolyBLEP to patches-dsp | P3 |
| T-0214 | Extract ADSR core ramp logic to patches-dsp | P3 |
| T-0215 | Extract noise PRNG and spectral shaping filters to patches-dsp | P3 |
| T-0216 | Add T6 SNR tests for MonoBiquad and SvfKernel | P4 |
| T-0217 | Add T4 stability tests for HalfbandInterpolator and DelayBuffer | P4 |
| T-0218 | Add T9 golden-file test for FDN reverb | P4 |
