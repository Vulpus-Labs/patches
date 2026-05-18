---
id: "0915"
title: Compressor + StereoCompressor
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Feed-forward compressor with soft knee, peak / RMS detection, and a
sidechain port. Stereo variant uses one linked detector
(`max(|L|, |R|)` peak or `sqrt((L² + R²) / 2)` RMS) and applies a
single gain-reduction value to both channels — independent L/R
detection would shift the stereo image under transients and is
explicitly out.

DSP kernel (`CompDetector` or similar) lives in
`patches-modules/src/dynamics/common/`. Kernel takes detector input
sample → gain-reduction sample. Module wraps it with parameter
routing, sidechain self-key per ticket 0914, and the linked-detector
input mixer for the stereo variant.

## Parameters

| Name | Type | Range | Default | Description |
|------|------|-------|---------|-------------|
| `threshold` | float | −60..0 dB | `-12` | Knee centre |
| `ratio` | float | 1..∞ | `4` | Above-knee slope; ∞ = limiter |
| `knee_width` | float | 0..24 dB | `6` | `0` = hard knee |
| `attack` | float | 0.1..1000 ms | `10` | Detector attack |
| `release` | float | 1..5000 ms | `100` | Detector release |
| `makeup` | float | −24..24 dB | `0` | Output gain trim |
| `detect` | enum | `peak` / `rms` | `peak` | Detection mode |
| `mix` | float | 0..1 | `1` | Dry/wet blend |

## Acceptance criteria

- [ ] `Compressor` and `StereoCompressor` registered in
      `patches-modules`; descriptors carry the ports listed in
      ADR 0076.
- [ ] Kernel test: detector follows ballistics within 5% of
      analytical attack/release curve.
- [ ] Kernel test: soft-knee output is C¹-continuous across the
      knee region (no kink at `threshold ± knee_width / 2`).
- [ ] Surface test: sidechain self-key fallback behaves as
      sidechain-fed-from-`in`.
- [ ] Surface test: `detect = rms` produces different gain reduction
      from `detect = peak` on a known transient.
- [ ] Stereo surface test: asymmetric L/R transient (e.g. spike on
      L only) produces identical gain reduction on L and R outputs.
- [ ] Manual page added under `docs/src/modules/`.
- [ ] `just commit -p patches-modules` green.

## Notes

`ratio = ∞` should saturate cleanly into a limiter-shaped curve;
property test the asymptote rather than special-casing.

`mix = 0` returns dry input unchanged (parallel-comp idiom uses
near-zero mix with high ratio). Verify the trivial passthrough.
