---
id: "0197"
title: Propagate periodic_update_interval to coefficient ramp calculations
priority: high
created: 2026-03-26
epic: E036
depends-on: "0195"
---

## Summary

`MonoBiquad::begin_ramp()` and `MonoSvfKernel::begin_ramp()` divide the delta between current and target coefficients by the hardcoded constant `COEFF_UPDATE_INTERVAL_RECIPROCAL = 1.0/32.0`. At 2× oversampling the interval is 64, so the ramp should complete over 64 samples. Change these kernels to accept the precomputed reciprocal as a parameter. Each module caches `interval_reciprocal: f32` (computed once in `prepare()`) and passes it at every `begin_ramp()` call, avoiding any per-call division.

## Acceptance criteria

- [ ] `MonoBiquad::begin_ramp(interval_recip: f32)` takes the precomputed reciprocal; `COEFF_UPDATE_INTERVAL_RECIPROCAL` is removed.
- [ ] `MonoSvfKernel::begin_ramp(interval_recip: f32)` likewise.
- [ ] Poly equivalents (`PolyBiquad`, `PolySvfKernel` if they exist) updated consistently.
- [ ] Every module implementing `PeriodicUpdate` stores `interval_recip: f32` (defaulting to `1.0 / BASE_PERIODIC_UPDATE_INTERVAL as f32`) and sets it from `1.0 / env.periodic_update_interval as f32` in `Module::prepare()`:
  - `ResonantLowpass`, `ResonantHighpass`, `ResonantBandpass` (`patches-modules/src/filter.rs`)
  - `Svf` (`patches-modules/src/svf.rs`)
  - `PolyResonantLowpass`, `PolyResonantHighpass`, `PolyResonantBandpass` (`patches-modules/src/poly_filter.rs`)
  - `PolySvf` (`patches-modules/src/poly_svf.rs`)
  - `FdnReverb` (`patches-modules/src/fdn_reverb.rs`)
  - Any other `PeriodicUpdate` implementors found during implementation
- [ ] `TimingShim` in `patches-profiling` updated if it delegates `prepare()` to an inner module.
- [ ] `cargo build`, `cargo test`, and `cargo clippy` all pass with zero warnings.

## Notes

`COEFF_UPDATE_INTERVAL_RECIPROCAL` is defined in `patches-modules/src/common/mono_biquad.rs`. After this ticket it can be deleted entirely.

The reciprocal is computed once in `prepare()` and reused on every `periodic_update()` call:

```rust
// in prepare():
self.interval_recip = 1.0 / env.periodic_update_interval as f32;

// in periodic_update():
self.kernel.begin_ramp(target_coeffs, self.interval_recip);
```

The interval is always ≥ 32 and a power of two, so the float conversion is exact.
