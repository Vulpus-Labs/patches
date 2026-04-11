---
id: "0333"
title: Claves module
priority: medium
created: 2026-04-12
---

## Summary

Implement a self-contained claves module. Claves produce a short, bright,
resonant "click" — essentially a sharply decaying bandpass-filtered impulse.
Simple synthesis but with enough parameters to dial in rimshots, woodblocks,
and similar percussive tones.

## Design

Signal path:
1. Short impulse (single-sample or very short noise burst) on trigger.
2. Fed into a high-Q bandpass SVF at the target pitch.
3. Shaped by a fast `DecayEnvelope`.

The SVF resonance does most of the tonal work — the impulse excites it
and the filter rings at the set frequency.

### Inputs

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

### Outputs

| Port  | Kind | Description    |
| ----- | ---- | -------------- |
| `out` | mono | Claves signal  |

### Parameters

| Name      | Type  | Range          | Default | Description                       |
| --------- | ----- | -------------- | ------- | --------------------------------- |
| `pitch`   | float | 200–5000 Hz    | 2500    | Resonant frequency                |
| `decay`   | float | 0.01–0.5 s     | 0.06    | Amplitude decay time              |
| `reson`   | float | 0.3–1.0        | 0.85    | Bandpass resonance / ring         |

## Acceptance criteria

- [ ] Module registered as `Claves` in `default_registry()`.
- [ ] Produces a short, bright, pitched click on trigger.
- [ ] `pitch` controls the tone — output frequency tracks the parameter.
- [ ] Higher `reson` values produce a longer, more resonant ring.
- [ ] Doc comment follows module documentation standard.
- [ ] Unit tests: trigger response, pitch tracking, decay behaviour.
- [ ] `cargo clippy` clean, no `unwrap()`/`expect()`.

## Notes

Depends on: 0327 (DecayEnvelope)
Uses existing `SvfKernel` for bandpass.
Epic: E061
