---
id: "0178"
title: Introduce PeriodicUpdate trait for coefficient recalculation
priority: medium
created: 2026-03-22
---

## Summary

Modules that interpolate CV-modulated coefficients (filters, SVFs) currently
carry an `update_counter` field in their kernel structs and branch on
`should_update()` inside every `process()` call. This is unnecessary noise in
the hot path: within a ~128-sample audio callback buffer, jitter caused by all
modules recalculating on the same sample is inaudible because it only matters
whether the *total* budget for the buffer is exceeded. Moving periodic
recalculation out of `process()` and into a dedicated trait called by the
execution plan at fixed 32-sample boundaries simplifies the kernel structs,
removes per-module branches from the hot path, and makes the recalculation
schedule explicit and predictable.

## Acceptance criteria

- [ ] `PeriodicUpdate` trait added to `patches-core`:
  ```rust
  pub trait PeriodicUpdate {
      fn periodic_update(&mut self, pool: &CablePool<'_>);
  }
  ```
- [ ] `Module` trait gains a default method:
  ```rust
  fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> { None }
  ```
- [ ] `ExecutionPlan` gains `periodic_indices: Vec<usize>` and
  `sample_counter: u32`. The planner calls `as_periodic()` on each module
  during plan activation and records indices of those that return `Some`.
- [ ] `ExecutionPlan::tick()` increments `sample_counter` (wrapping at
  `COEFF_UPDATE_INTERVAL`) and, when it reaches zero, iterates
  `periodic_indices` and calls `periodic_update(pool)` on each before the
  main processing loop.
- [ ] `update_counter`, `should_update()`, and `advance_counter()` removed
  from `MonoBiquad`, `PolyBiquad`, `MonoSvfKernel`, and `PolySvfKernel`.
- [ ] `should_update()` branches and counter increments removed from all
  `process()` implementations in `patches-modules`.
- [ ] All existing tests pass; `cargo clippy` produces no new warnings.

## Affected modules

The following modules in `patches-modules` will implement `PeriodicUpdate`:

- `Filter` / `PolyFilter` — `MonoBiquad` / `PolyBiquad` absorption coefficient interpolation
- `Svf` / `PolySvf` — `MonoSvfKernel` / `PolySvfKernel` coefficient interpolation
- `FdnReverb` (T-0176, currently in progress) — eight `MonoBiquad` absorption
  filters; T-0176 currently specifies the same `COEFF_UPDATE_INTERVAL = 32`
  cadence via an in-`process()` counter. That ticket should be implemented
  against this trait instead, or updated post-hoc if T-0176 lands first.

## Notes

`periodic_update` receives a read-only pool reference to read CV input values
(the previous sample's values, consistent with the 1-sample cable delay).
Interpolation ramps (`begin_ramp` / `begin_ramp_voice`) remain in the kernels
and are called from `periodic_update`; the `tick()` / `tick_voice()` methods
continue to advance active coefficients by their deltas each sample.

The `as_periodic()` method on `Module` is the coupling cost: the core trait
becomes weakly aware of the periodic mechanism. The default no-op means
non-periodic modules pay nothing. The alternative (planner downcasts via
`as_any()`) is messier and was rejected.

`COEFF_UPDATE_INTERVAL` (32) moves logically to the execution plan; the
constant can stay in `patches-core` or `patches-modules` wherever it is most
natural after the refactor.
