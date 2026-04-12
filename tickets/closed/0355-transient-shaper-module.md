---
id: "0355"
title: TransientShaper module
priority: medium
created: 2026-04-12
---

## Summary

Add a `TransientShaper` module that uses dual envelope followers to independently boost or cut attack transients and sustained tails. The envelope follower DSP goes in `patches-dsp` as a reusable `EnvelopeFollower` primitive.

## Design

**Algorithm — dual envelope follower:**

1. **Fast envelope:** Short attack/release time (from `speed` param). Tracks transients closely.
2. **Slow envelope:** Longer time constant (~4x `speed`). Tracks the sustained level.
3. **Transient signal** = fast - slow (positive during attacks, near-zero during sustain).
4. **Sustain signal** = slow envelope.
5. **Gain** = `1.0 + attack * transient_signal + sustain * sustain_signal`
6. **Output** = input * gain, mixed with dry.

**Envelope follower (`EnvelopeFollower` in `patches-dsp`):**

```text
envelope += coeff * (|input| - envelope)
```

With separate attack/release coefficients: attack coeff is larger (fast rise), release coeff is smaller (slower decay). Coefficients computed from time in ms using the exponential smoothing formula: `coeff = 1.0 - exp(-1.0 / (time_ms * 0.001 * sample_rate))`.

**Parameters:**

| Name      | Type  | Range        | Default | Description                                  |
| --------- | ----- | ------------ | ------- | -------------------------------------------- |
| `attack`  | float | -1.0 - 1.0  | 0.0     | Boost (+) or cut (-) transient attack        |
| `sustain` | float | -1.0 - 1.0  | 0.0     | Boost (+) or cut (-) sustained portion       |
| `speed`   | float | 1.0 - 100.0 | 20.0    | Detector speed in ms                         |
| `mix`     | float | 0.0 - 1.0   | 1.0     | Dry/wet blend                                |

**Ports:**

| Port  | Kind | Direction | Description      |
| ----- | ---- | --------- | ---------------- |
| `in`  | mono | input     | Audio input      |
| `out` | mono | output    | Processed output |

## Acceptance criteria

- [ ] `EnvelopeFollower` in `patches-dsp/src/envelope_follower.rs` with `tick(input) -> f32`
- [ ] Configurable attack/release time via `set_attack_ms` / `set_release_ms`
- [ ] Envelope follower unit tests: step response rises to target, decays back to zero
- [ ] Module registered as `TransientShaper`
- [ ] `attack=0, sustain=0` passes signal unchanged (unity gain)
- [ ] Positive `attack` audibly boosts transients (gain > 1.0 during onset)
- [ ] Negative `sustain` reduces tail level
- [ ] `speed` parameter affects how quickly the detector responds
- [ ] `speed` coefficients updated in `periodic_update`
- [ ] Doc comment follows module documentation standard
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- `EnvelopeFollower` is deliberately a standalone DSP primitive — it will be reusable for sidechain compression, auto-wah, ducking, and other envelope-dependent effects.
- The slow follower uses 4x the `speed` value. This ratio is a design choice that seems to work well for drum transients; it could become a parameter later if needed.
- At extreme `attack` boost values, output can exceed [-1, 1]. Users should follow with a limiter if needed — this module intentionally does not clip, to preserve dynamics.
