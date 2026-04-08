---
id: "0196"
title: Drive ExecutionState scheduling from AudioEnvironment::periodic_update_interval
priority: high
created: 2026-03-26
epic: E036
depends-on: "0195"
---

## Summary

`ExecutionState` currently wraps `sample_counter` at the compile-time constant `COEFF_UPDATE_INTERVAL` (32). Change it to wrap at `AudioEnvironment::periodic_update_interval` so that at 2× oversampling the scheduler fires every 64 oversampled samples (same wall-clock period as 32 samples at 1×).

## Acceptance criteria

- [ ] `ExecutionState` stores a `periodic_update_interval: u32` field (set once at construction or via a `set_interval` method called from the engine when the environment is established).
- [ ] `ExecutionState::tick()` wraps `sample_counter` at `self.periodic_update_interval` rather than `COEFF_UPDATE_INTERVAL`.
- [ ] The bitmask optimisation (`COEFF_UPDATE_INTERVAL_MASK`) is either replaced with a modulo operation or preserved as a local variable derived from the interval (valid only when the interval is a power of two, which it always is given the oversampling factor set).
- [ ] `AudioCallback` and `HeadlessEngine` pass the interval from `AudioEnvironment` to `ExecutionState` at plan activation time (i.e., when `rebuild()` is called).
- [ ] `cargo build`, `cargo test`, and `cargo clippy` all pass with zero warnings.

## Notes

`ExecutionState` lives in `patches-engine/src/execution_state.rs`. The interval is available from `AudioEnvironment` which is created in `SoundEngine::open()` and stored on `AudioCallback`. The simplest thread-safe path is to store it in `ExecutionState` and update it from `rebuild()` (called inside `receive_plan` / `adopt_plan`), passing the interval alongside the plan — or storing it in `AudioCallback` and reading it there.

The interval is always a power of two (32, 64, 128, 256), so the bitmask trick (`counter & (interval - 1)`) remains valid; compute the mask from the interval rather than from the constant.
