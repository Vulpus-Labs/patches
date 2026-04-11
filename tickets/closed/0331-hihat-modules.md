---
id: "0331"
title: Hi-hat modules (open + closed)
priority: medium
created: 2026-04-12
---

## Summary

Implement `ClosedHiHat` and `OpenHiHat` modules. Both share the same core
synthesis engine — metallic tone from inharmonic oscillators mixed with
highpass-filtered noise — but differ in decay time and mutual interaction.
The open hi-hat has a choke input so a closed hi-hat trigger can cut it
short, as on a real 808.

## Design

Signal path:
1. `MetallicTone` generator (6 inharmonic square oscillators).
2. Mixed with highpass-filtered white noise.
3. Shaped by a `DecayEnvelope`.

The two modules are separate structs but share the same synthesis logic.

### ClosedHiHat

#### Inputs

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

#### Outputs

| Port  | Kind | Description      |
| ----- | ---- | ---------------- |
| `out` | mono | Closed hat signal |

#### Parameters

| Name      | Type  | Range          | Default | Description                      |
| --------- | ----- | -------------- | ------- | -------------------------------- |
| `pitch`   | float | 100–8000 Hz    | 400     | Base frequency of metallic tone  |
| `decay`   | float | 0.005–0.2 s    | 0.04    | Amplitude decay time             |
| `tone`    | float | 0.0–1.0        | 0.5     | Metallic vs noise mix            |
| `filter`  | float | 2000–16000 Hz  | 8000    | Noise highpass cutoff            |

### OpenHiHat

#### Inputs

| Port      | Kind | Description                          |
| --------- | ---- | ------------------------------------ |
| `trigger` | mono | Rising edge triggers                 |
| `choke`   | mono | Rising edge chokes (cuts) the sound  |

#### Outputs

| Port  | Kind | Description    |
| ----- | ---- | -------------- |
| `out` | mono | Open hat signal |

#### Parameters

| Name      | Type  | Range          | Default | Description                      |
| --------- | ----- | -------------- | ------- | -------------------------------- |
| `pitch`   | float | 100–8000 Hz    | 400     | Base frequency of metallic tone  |
| `decay`   | float | 0.05–4.0 s     | 0.5     | Amplitude decay time             |
| `tone`    | float | 0.0–1.0        | 0.5     | Metallic vs noise mix            |
| `filter`  | float | 2000–16000 Hz  | 8000    | Noise highpass cutoff            |

## Acceptance criteria

- [ ] Both modules registered in `default_registry()`.
- [ ] Closed hat has short, tight decay; open hat has longer ring.
- [ ] Open hat's `choke` input silences a ringing hat immediately.
- [ ] `pitch` shifts the metallic tone spectrum.
- [ ] `tone` crossfades between metallic and noise components.
- [ ] Doc comments follow module documentation standard.
- [ ] Unit tests: trigger response, choke behaviour on open hat,
      decay time difference.
- [ ] `cargo clippy` clean, no `unwrap()`/`expect()`.

## Notes

Depends on: 0327 (MetallicTone, DecayEnvelope)
Uses existing `SvfKernel` for highpass filter.
Epic: E061
