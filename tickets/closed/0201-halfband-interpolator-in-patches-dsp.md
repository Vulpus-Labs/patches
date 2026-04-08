---
id: "0201"
title: Add HalfbandInterpolator to patches-dsp
priority: medium
created: 2026-03-26
epic: E037
depends_on: "0200"
---

## Summary

Add `patches-dsp::HalfbandInterpolator` — the inverse of the decimation path.
Given one base-rate input sample it produces two oversampled output samples via
zero-insertion followed by the halfband FIR low-pass filter.

This is the detector-side upsampler for the lookahead limiter (and any other module
that needs inter-sample peak estimation).

## Design

**Zero-insertion interpolation:**
- For each input sample `x[n]`, push `x[n]` then `0.0` into the FIR delay line.
- Evaluate the FIR at both positions to get two oversampled output samples.
- Scale both outputs by `2.0` to compensate for the 0.5× gain introduced by
  zero-insertion (the halfband coefficients sum to ~0.5 at DC).

**Group delay:** The interpolator shares the same filter kernel as the decimator and
therefore introduces the same group delay: `HalfbandFir::GROUP_DELAY_OVERSAMPLED`
samples at the oversampled rate (= `GROUP_DELAY_OVERSAMPLED / 2` at base rate, i.e.
8 samples for the default taps).

**API:**

```rust
pub struct HalfbandInterpolator { /* wraps HalfbandFir */ }

impl HalfbandInterpolator {
    pub fn default() -> Self;
    pub fn new(fir: HalfbandFir) -> Self;

    /// Feed one base-rate sample; returns two oversampled samples [a, b].
    /// a corresponds to the even (real) position, b to the odd (interpolated) position.
    pub fn process(&mut self, x: f32) -> [f32; 2];

    /// Group delay of the interpolator in oversampled samples.
    pub const GROUP_DELAY_OVERSAMPLED: usize;

    /// Group delay in base-rate samples (GROUP_DELAY_OVERSAMPLED / 2).
    pub const GROUP_DELAY_BASE_RATE: usize;
}
```

## Acceptance criteria

- [ ] `HalfbandInterpolator::default()` constructs with the default taps.
- [ ] DC input (constant `1.0`) converges to `[1.0, 1.0]` per pair after settling
      (within 0.01 tolerance). Test included.
- [ ] Nyquist input at the base rate (`+1, -1` alternating) produces near-zero
      output at both oversampled positions after settling. Test included.
- [ ] A 1 kHz sine at 48 kHz base rate produces oversampled output with amplitude
      within 0.1 dB of the input (passband test). Test included.
- [ ] `GROUP_DELAY_BASE_RATE == HalfbandFir::GROUP_DELAY_OVERSAMPLED / 2`.
- [ ] `cargo test` and `cargo clippy` pass with 0 warnings.

## Notes

- The `HalfbandFir::process(first, second)` method already accepts two samples and
  produces one output. For interpolation you call it **twice per input sample**:
  once with `(x, 0.0)` and once with `(0.0, 0.0)` — but you need to feed the FIR's
  delay line correctly. Concretely: push `x` and evaluate; push `0.0` and evaluate;
  scale both by 2.
- Alternatively, `HalfbandFir` could expose a lower-level push+evaluate API to avoid
  computing the convolution sum twice in a row. That's an internal implementation
  detail — do what makes the code cleanest without over-engineering.
- Do not expose `HalfbandInterpolator` from `patches-engine`; it belongs purely in
  `patches-dsp`.
