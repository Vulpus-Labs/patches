---
id: "0200"
title: Extract HalfbandFir kernel from patches-engine into patches-dsp
priority: high
created: 2026-03-26
epic: E037
depends_on: "0199"
---

## Summary

Move the core halfband FIR filter logic out of `patches-engine/src/decimator.rs` and
into `patches-dsp`. The `Decimator` enum and its `X2Stage` wrapper stay in
`patches-engine` (they are engine-specific), but the underlying filter struct and
default coefficients become a public, reusable type in `patches-dsp`.

## Background

`HalfbandDecimator` in `patches-engine` is `pub(crate)`. Its internals (the 33-tap
filter, default coefficients) are exactly what `patches-dsp::HalfbandInterpolator`
(T-0201) will reuse. Extracting the kernel now avoids duplication.

The filter is a symmetric linear-phase FIR. Non-zero off-centre taps are supplied at
construction; every other tap is zero by the halfband property. The centre tap is
fixed. Full filter length = 4 × n_taps + 1 = 33; group delay = 16 samples at the
oversampled rate = **8 samples at base rate**.

## What to do

1. Create `patches-dsp/src/halfband.rs` containing:
   - `DEFAULT_TAPS: [f32; 8]` and `DEFAULT_CENTRE: f32` (copy from decimator.rs).
   - `pub struct HalfbandFir` — rename from `HalfbandDecimator`; keep the same
     fields and algorithm.
   - `impl HalfbandFir` with a `pub fn new(taps: Vec<f32>, centre: f32) -> Self`
     and a `pub fn process(&mut self, first: f32, second: f32) -> f32` method.
   - `pub const GROUP_DELAY_OVERSAMPLED: usize` — the group delay in oversampled
     samples `(4 * n_taps) / 2` (= 16 for the default taps). Document it.
   - `pub fn default() -> Self` constructing with `DEFAULT_TAPS` / `DEFAULT_CENTRE`.
2. Add `mod halfband; pub use halfband::HalfbandFir;` to `patches-dsp/src/lib.rs`.
3. Add `patches-dsp` as a dependency of `patches-engine` in its `Cargo.toml`
   (path dependency, no version needed since we're in the same workspace).
4. In `patches-engine/src/decimator.rs`, remove `HalfbandDecimator` and update
   `X2Stage` to hold a `patches_dsp::HalfbandFir` instead. Keep `X2Stage`,
   `Decimator`, their tests, and `DEFAULT_TAPS`/`DEFAULT_CENTRE` references
   removed (now in patches-dsp).
5. Migrate the `HalfbandDecimator` unit tests (impulse response, DC, Nyquist) into
   `patches-dsp/src/halfband.rs` as `#[cfg(test)]` tests on `HalfbandFir`.

## Acceptance criteria

- [ ] `patches-dsp::HalfbandFir` is public and constructible with `HalfbandFir::default()`.
- [ ] `patches-engine` no longer defines `HalfbandDecimator`; `X2Stage` uses
      `patches_dsp::HalfbandFir`.
- [ ] The three diagnostic tests (impulse response, DC response, Nyquist response) exist
      in `patches-dsp` and pass.
- [ ] `cargo test` passes across all crates with 0 clippy warnings.

## Notes

- Keep `DEFAULT_TAPS` and `DEFAULT_CENTRE` **in `patches-dsp`** only; do not leave
  a copy in `patches-engine`.
- `GROUP_DELAY_OVERSAMPLED` is a named constant rather than a magic number so that
  both the decimator and the interpolator (T-0201) can reference it when sizing delay
  lines and peak windows.
