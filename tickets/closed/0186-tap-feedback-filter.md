---
id: "0186"
title: TapFeedbackFilter and ToneFilter helpers
epic: "E034"
priority: medium
created: 2026-03-24
---

## Summary

Add two small helper structs to `patches-modules/src/common/` used by both
`Delay` and `StereoDelay`. Extracting them first keeps the module tickets focused
on signal routing rather than filter arithmetic.

## Acceptance criteria

- [ ] `TapFeedbackFilter` in `patches-modules/src/common/tap_feedback_filter.rs`:
  - Fields: `x_prev: f32`, `dc_y_prev: f32`, `hf_y_prev: f32`, `r: f32`, `alpha: f32`
  - `fn new() -> Self` — zeroed state, placeholder coefficients
  - `fn prepare(&mut self, sample_rate: f32)` — computes `r = 1.0 - 2π·5/sample_rate`
    and `alpha = 1.0 - (-2π·16000/sample_rate).exp()`
  - `fn reset(&mut self)` — zeroes all state fields (call when module reinitialises)
  - `fn process(&mut self, x: f32, drive: f32) -> f32` — applies in order:
    1. scale: `x *= drive`
    2. DC block: `y = x − self.x_prev + self.r · self.dc_y_prev`; update `x_prev`, `dc_y_prev`
    3. HF limit: `y = self.hf_y_prev + self.alpha · (y − self.hf_y_prev)`; update `hf_y_prev`
    4. return `fast_tanh(y)`
  - No allocations; all state is inline `f32`

- [ ] `ToneFilter` in `patches-modules/src/common/tone_filter.rs`:
  - A one-pole lowpass whose cutoff is parameterised by a `tone` value in [0, 1]
  - `fn new() -> Self`
  - `fn prepare(&mut self, sample_rate: f32)` — stores sample rate for coefficient computation
  - `fn set_tone(&mut self, tone: f32)` — recomputes and caches `alpha`; called from
    `update_validated_parameters`, not from `process`; at `tone = 1.0` sets alpha to 1.0
    (passthrough); at `tone = 0.0` rolls off to ~200 Hz; log mapping:
    `cutoff_hz = 200.0 * (sample_rate / 2.0 / 200.0).powf(tone)`
    `alpha = 1.0 - (-2π · cutoff_hz / sample_rate).exp()`
  - `fn process(&mut self, x: f32) -> f32` — `y = self.y_prev + self.alpha * (x - self.y_prev)`;
    no coefficient computation on the audio thread
  - No allocations

- [ ] Both structs exported from `patches-modules::common` (add `pub mod` entries in
  `patches-modules/src/common/mod.rs`)
- [ ] Unit tests:
  - `TapFeedbackFilter`: DC input converges to zero; impulse response bounded to (−1, 1)
    for any drive value; output of `fast_tanh` at `drive = 10.0` stays in (−1, 1)
  - `ToneFilter`: at `tone = 1.0` a 10 kHz sine passes with < 0.1 dB attenuation;
    at `tone = 0.0` a 10 kHz sine is attenuated by > 20 dB
- [ ] `cargo clippy` clean; `cargo test -p patches-modules` passes

## Notes

The `alpha` recomputation on every call in `ToneFilter` costs one `exp`. If profiling
shows this is a bottleneck, the caller can cache the coefficient and only recompute when
the tone parameter changes (same pattern as `PeriodicUpdate` in `FdnReverb`). Leave
that optimisation for a follow-up ticket.

The DC block cutoff (5 Hz) and HF limit cutoff (16 kHz) are not user-facing; they are
hardcoded in `prepare`. If there is a later need to expose them as parameters, they can
be factored into `TapFeedbackFilter::prepare_with(dc_hz, hf_hz, sample_rate)` without
breaking existing callers.
