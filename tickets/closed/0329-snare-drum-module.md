---
id: "0329"
title: Snare drum module
priority: high
created: 2026-04-12
---

## Summary

Implement a self-contained snare drum module. The snare combines a tuned
body oscillator with a filtered noise burst, each with independent decay
envelopes. Broadly 808-flavoured but with enough parameters to range from
tight electronic snaps to loose, ringy snares.

## Design

Two signal paths mixed together:

1. **Body**: sine oscillator with a short pitch sweep and amplitude decay.
2. **Noise**: white noise through a bandpass filter (SVF) with its own
   amplitude decay.

The `tone` parameter crossfades between body and noise.

### Inputs

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

### Outputs

| Port  | Kind | Description   |
| ----- | ---- | ------------- |
| `out` | mono | Snare signal  |

### Parameters

| Name          | Type  | Range         | Default | Description                           |
| ------------- | ----- | ------------- | ------- | ------------------------------------- |
| `pitch`       | float | 80–400 Hz     | 180     | Body oscillator base pitch            |
| `tone`        | float | 0.0–1.0       | 0.5     | Body vs noise mix (0 = all body)      |
| `body_decay`  | float | 0.01–1.0 s    | 0.15    | Body amplitude decay time             |
| `noise_decay` | float | 0.01–1.0 s    | 0.2     | Noise amplitude decay time            |
| `noise_freq`  | float | 500–10000 Hz  | 3000    | Noise bandpass centre frequency       |
| `noise_q`     | float | 0.0–1.0       | 0.3     | Noise bandpass resonance              |
| `snap`        | float | 0.0–1.0       | 0.5     | Transient snap intensity              |

## Acceptance criteria

- [ ] Module registered as `Snare` in `default_registry()`.
- [ ] Produces audible output on trigger.
- [ ] `tone` at 0 produces mostly tonal body; at 1 produces mostly noise.
- [ ] `pitch` affects body oscillator frequency.
- [ ] Noise path is bandpass-filtered — `noise_freq` shifts the spectral
      peak.
- [ ] Doc comment follows module documentation standard.
- [ ] Unit tests: trigger response, decay behaviour, tone mix extremes.
- [ ] `cargo clippy` clean, no `unwrap()`/`expect()`.

## Notes

Depends on: 0327 (DecayEnvelope, PitchSweep, saturate)
Uses existing `SvfKernel` from `patches-dsp` for the noise bandpass.
Epic: E061
