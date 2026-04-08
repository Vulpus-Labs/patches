# E037 — patches-dsp: shared DSP primitives crate

## Goal

Introduce a new `patches-dsp` crate that acts as the home for reusable DSP building
blocks. Both `patches-engine` (decimator) and `patches-modules` (module
implementations) need shared filter/buffer infrastructure; the current arrangement
duplicates this implicitly by keeping everything in `patches-engine`'s private scope
or in `patches-modules/src/common/`.

The initial scope is narrow: extract the halfband FIR filter kernel, add its inverse
(an interpolator), move `DelayBuffer` out of modules-common, and add a `PeakWindow`
primitive. The `common/` modules that remain in `patches-modules` (biquads, SVF
kernels, etc.) are deferred to future epics.

## Background

The immediate driver is a lookahead limiter module that needs:

1. A halfband FIR **interpolator** (upsample ×2, insert zeros, filter) to detect
   inter-sample peaks. The same filter coefficients live in `patches-engine`'s
   `HalfbandDecimator`, but that type is `pub(crate)` and unavailable to
   `patches-modules`.
2. A **`PeakWindow`** that tracks the rolling maximum absolute value of the
   oversampled stream over the filter's group-delay window.
3. A **dry-path delay line** (`DelayBuffer`) to time-align the original-rate signal
   with the oversampled peak measurement.

Rather than duplicate the FIR kernel a second time inside `patches-modules`, the
cleaner answer is a shared crate.

## Dependency graph after this epic

```
patches-core
    ↑
patches-dsp          ← new (no audio-backend deps)
    ↑           ↑
patches-engine  patches-modules
```

`patches-dsp` depends only on `patches-core` (for any shared types it needs) and
nothing else; it must remain free of CPAL, serde, or other heavy dependencies.

## Tickets

| # | Title |
|---|-------|
| T-0199 | Create patches-dsp crate scaffold |
| T-0200 | Extract HalfbandFir kernel from patches-engine into patches-dsp |
| T-0201 | Add HalfbandInterpolator to patches-dsp |
| T-0202 | Move DelayBuffer from patches-modules/common to patches-dsp |
| T-0203 | Add PeakWindow to patches-dsp |
