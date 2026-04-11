---
id: "0327"
title: Drum synthesis DSP primitives in patches-dsp
priority: high
created: 2026-04-12
---

## Summary

Add reusable DSP building blocks for drum synthesis to `patches-dsp`. These
primitives are stateless or minimal-state kernels with no module-level
coupling, following the same pattern as `AdsrCore`, `SvfKernel`, etc.

## Primitives

### DecayEnvelope

A single-stage exponential decay triggered by a rising edge. Simpler than
`AdsrCore` for drum sounds that only need attack-decay behaviour.

- Configurable decay time (seconds) and curve shape (linear / exponential).
- `tick(trigger) -> f32` returns envelope level.
- Retrigger resets to 1.0 on rising edge.

### PitchSweep

Exponential pitch sweep from a start frequency to an end frequency over a
configurable time. Used for kick and tom body pitch envelopes.

- `set_params(start_hz, end_hz, sweep_time_secs, sample_rate)`
- `trigger()` resets sweep to start.
- `tick() -> f32` returns current frequency in Hz.

### Waveshaper

Soft-clipping / saturation function for adding grit to kicks and snares.

- `saturate(sample, drive) -> f32` — drive in [0, 1] maps from clean to
  hard clip via tanh-like curve.

### MetallicTone

Generates a metallic timbre by summing several square/pulse oscillators at
inharmonic frequency ratios. Used for hi-hats and cymbals.

- Configurable base frequency and number of partials (up to 6).
- Fixed ratio table (e.g. 1.0, 1.4471, 1.6170, 1.9265, 2.5028, 2.6637 —
  classic metallic ratios).
- `tick() -> f32` returns summed output.
- `trigger(freq_hz)` resets phase.

### BurstGenerator

Generates a sequence of short noise bursts with configurable spacing,
used for clap synthesis.

- `set_params(burst_count, burst_spacing_samples, burst_decay)`
- `trigger()` starts the burst sequence.
- `tick(noise_sample) -> f32` gates and envelopes the input noise.

## Acceptance criteria

- [ ] `DecayEnvelope` implemented with tests: trigger response, decay
      shape, retrigger behaviour.
- [ ] `PitchSweep` implemented with tests: start/end frequency accuracy,
      sweep timing.
- [ ] `saturate()` function implemented with tests: unity at zero drive,
      symmetry, bounded output.
- [ ] `MetallicTone` implemented with tests: output is non-silent after
      trigger, frequency ratios are present in spectrum.
- [ ] `BurstGenerator` implemented with tests: correct burst count and
      spacing.
- [ ] All code in `patches-dsp/src/`, no module or patches-core coupling
      beyond what `patches-dsp` already has.
- [ ] `cargo test -p patches-dsp` passes.

## Notes

Epic: E061
