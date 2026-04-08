---
id: "0205"
title: Integration tests for Limiter module
priority: medium
created: 2026-03-26
depends_on: "0204"
---

## Summary

Add integration tests for the `Limiter` module in `patches-integration-tests`,
covering the key behavioural invariants: gain reduction on sustained overdrive,
inter-sample peak detection, and gain recovery after a transient.

## Test cases

### 1. `limiter_clamps_sustained_overdrive`

Feed a full-scale sine wave (`amplitude = 1.2`, threshold = 1.0). After an
appropriate settling period (≥ release time), verify that every output sample
satisfies `|output| ≤ threshold * 1.01` (1% headroom for filter transients).

This is the basic correctness test.

### 2. `limiter_catches_intersample_peak`

Construct a pair of samples whose interpolated midpoint exceeds the threshold even
though both samples themselves are below it. A simple case: two adjacent samples at
`sin(π/2 - ε)` and `sin(π/2 + ε)` with threshold set just below the true peak
`sin(π/2) = 1.0`. The limiter should apply gain reduction; a sample-domain-only
limiter would not.

Use a simple analytical construction: with 48 kHz base rate and a sine slightly above
the Nyquist of 24 kHz, adjacent sample values straddle a peak. Alternatively,
synthesise two samples `[0.95, 0.95]` with an interpolated peak known to exceed 1.0
for a near-Nyquist partial.

The test should verify that gain reduction is applied (i.e. `output < input` for the
relevant samples) — it does not need to verify the exact gain value.

### 3. `limiter_gain_recovers_after_transient`

Feed one loud transient (a single sample at `2.0`, threshold = 1.0) followed by
silence. Verify:
- The transient output is ≤ threshold (gain reduction applied).
- After `release_ms` milliseconds of silence, `current_gain` has recovered to
  within 1% of 1.0 (i.e. release is working).

Since the module's `current_gain` is internal, measure recovery indirectly: inject a
second small probe sample (`0.5`) after the release window and confirm it passes
through at close to unity gain (output ≈ 0.5 ± 0.01).

### 4. `limiter_unity_gain_below_threshold`

Feed a sine wave at 50% amplitude (well below threshold). Verify that all output
samples match the delayed input within floating-point tolerance, i.e. no gain
reduction is applied and the only effect is the fixed `GROUP_DELAY_BASE_RATE` sample
latency.

## Notes

- Use `HeadlessEngine` from `patches-integration-tests/src/lib.rs` to run the
  engine without audio hardware.
- All tests must be non-`#[ignore]` (no hardware dependency).
- The delay offset in test 4 must account for `HalfbandInterpolator::GROUP_DELAY_BASE_RATE`
  samples of latency when comparing input to output.
- Test file: `patches-integration-tests/tests/limiter.rs`.
