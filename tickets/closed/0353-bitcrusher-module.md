---
id: "0353"
title: Bitcrusher module and DSP kernel
priority: medium
created: 2026-04-12
---

## Summary

Add a `Bitcrusher` effect module for sample-rate reduction and bit-depth reduction. The DSP kernel lives in `patches-dsp` as a standalone `BitcrusherKernel` so it can be unit-tested without module infrastructure.

## Design

**Rate reduction:** Sample-and-hold with a fractional phase accumulator. Phase increments by `effective_rate / sample_rate` each tick. When phase wraps past 1.0, capture a new input sample; otherwise hold the previous value. This gives smooth rate sweeps without clicks.

**Bit reduction:** `quantize(x) = round(x * levels) / levels` where `levels = 2^depth`. Continuous (non-integer) depth values are supported for smooth degradation.

**Parameters:**

| Name    | Type  | Range      | Default | Description                                                  |
| ------- | ----- | ---------- | ------- | ------------------------------------------------------------ |
| `rate`  | float | 0.0 - 1.0 | 1.0     | Rate reduction (1.0 = full rate, 0.0 = ~100 Hz effective). Log mapping. |
| `depth` | float | 1.0 - 32.0 | 32.0   | Bit depth (continuous)                                       |
| `dry_wet` | float | 0.0 - 1.0 | 1.0   | Dry/wet mix                                                  |

**Ports:**

| Port       | Kind | Direction | Description          |
| ---------- | ---- | --------- | -------------------- |
| `in`       | mono | input     | Audio input          |
| `rate_cv`  | mono | input     | Rate modulation      |
| `depth_cv` | mono | input     | Depth modulation     |
| `out`      | mono | output    | Processed output     |

## Acceptance criteria

- [ ] `BitcrusherKernel` in `patches-dsp/src/bitcrusher.rs` with `tick(input) -> f32`
- [ ] `set_rate(rate, sample_rate)` and `set_depth(depth)` configuration methods
- [ ] Module registered as `Bitcrusher` in `patches-modules`
- [ ] rate=1.0 + depth=32.0 passes signal unchanged (identity test)
- [ ] rate=0.0 holds samples for audible staircase effect
- [ ] depth=1.0 produces 1-bit quantisation (signal snaps to ±0.5 boundaries)
- [ ] CV inputs processed in `periodic_update`
- [ ] Doc comment follows module documentation standard
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- Log mapping for rate: `effective_rate = 100.0 * (sample_rate / 100.0).powf(rate)` gives a perceptually linear sweep from ~100 Hz to full sample rate.
- The kernel should be zero-allocation — just a held sample, phase accumulator, and cached levels value.
