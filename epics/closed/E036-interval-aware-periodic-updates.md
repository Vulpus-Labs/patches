# E036 — Interval-aware periodic updates

## Goal

`AudioEnvironment` gains a `periodic_update_interval` field that scales with the oversampling factor: 32 at 1×, 64 at 2×, 128 at 4×, 256 at 8×. The execution scheduler and all coefficient-ramping modules consume this value, ensuring filter and SVF smoothing ramps span a constant wall-clock duration regardless of the active oversampling factor.

## Background

`COEFF_UPDATE_INTERVAL = 32` is a compile-time constant used in two places:

1. **`ExecutionState`** — `sample_counter` wraps at this value, controlling how often `periodic_update()` is called on `PeriodicUpdate` modules.
2. **Coefficient kernels** (`MonoBiquad`, `MonoSvfKernel`) — `begin_ramp()` divides the distance to the target by this constant to compute per-sample deltas.

At 2× oversampling the engine processes 96 000 samples/s. Keeping the interval at 32 means periodic updates fire ~3000×/s (twice as often) and coefficient ramps complete in half the wall-clock time, degrading smoothing quality and wasting CPU.

The fix is to derive the interval from the oversampling factor at engine open time and thread it through wherever the constant is used.

## Tickets

| # | Title |
|---|-------|
| T-0195 | Add `periodic_update_interval` to `AudioEnvironment` |
| T-0196 | Drive `ExecutionState` scheduling from `AudioEnvironment::periodic_update_interval` |
| T-0197 | Propagate `periodic_update_interval` to coefficient ramp calculations |
| T-0198 | Integration tests for interval scaling at 2× oversampling |
