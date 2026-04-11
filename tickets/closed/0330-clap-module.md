---
id: "0330"
title: Clap module
priority: medium
created: 2026-04-12
---

## Summary

Implement a self-contained clap module. The classic 808 clap is a burst of
filtered noise repeats followed by a longer decay tail. This module
reproduces that structure with configurable burst count, spacing, filter
frequency, and decay.

## Design

White noise passed through a bandpass filter, gated by the `BurstGenerator`
to produce the initial "clappy" retriggered transient, then shaped by a
longer `DecayEnvelope` for the tail.

### Inputs

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

### Outputs

| Port  | Kind | Description  |
| ----- | ---- | ------------ |
| `out` | mono | Clap signal  |

### Parameters

| Name        | Type  | Range        | Default | Description                         |
| ----------- | ----- | ------------ | ------- | ----------------------------------- |
| `decay`     | float | 0.05–2.0 s   | 0.3     | Tail decay time                     |
| `filter`    | float | 500–8000 Hz  | 1200    | Bandpass centre frequency           |
| `q`         | float | 0.0–1.0      | 0.4     | Bandpass resonance                  |
| `spread`    | float | 0.0–1.0      | 0.5     | Spacing between bursts              |
| `bursts`    | int   | 1–8          | 4       | Number of noise bursts              |

## Acceptance criteria

- [ ] Module registered as `Clap` in `default_registry()`.
- [ ] Produces audible output on trigger with recognisable "clap"
      character — multiple transients followed by a tail.
- [ ] `bursts` and `spread` parameters visibly affect the initial
      transient pattern.
- [ ] `filter` shifts the spectral centre of the noise.
- [ ] Doc comment follows module documentation standard.
- [ ] Unit tests: trigger response, burst count reflected in output
      envelope shape.
- [ ] `cargo clippy` clean, no `unwrap()`/`expect()`.

## Notes

Depends on: 0327 (DecayEnvelope, BurstGenerator)
Uses existing `SvfKernel` for bandpass.
Epic: E061
