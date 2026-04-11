---
id: "0334"
title: Cymbal module
priority: medium
created: 2026-04-12
---

## Summary

Implement a self-contained cymbal module for crash and ride sounds. Cymbals
use the same metallic tone engine as hi-hats but with more partials, a
higher frequency range, longer decay, and a "shimmer" parameter that adds
subtle pitch modulation to the partials for a more animated, washy sound.

## Design

Signal path:
1. `MetallicTone` generator (6 inharmonic oscillators) at a higher base
   frequency than hi-hats.
2. Mixed with highpass-filtered white noise.
3. Optional slow LFO modulation of partial frequencies for shimmer.
4. Shaped by a `DecayEnvelope` with a long range.

### Inputs

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

### Outputs

| Port  | Kind | Description     |
| ----- | ---- | --------------- |
| `out` | mono | Cymbal signal   |

### Parameters

| Name      | Type  | Range          | Default | Description                         |
| --------- | ----- | -------------- | ------- | ----------------------------------- |
| `pitch`   | float | 200–10000 Hz   | 600     | Base frequency of metallic tone     |
| `decay`   | float | 0.2–8.0 s      | 2.0     | Amplitude decay time                |
| `tone`    | float | 0.0–1.0        | 0.5     | Metallic vs noise mix               |
| `filter`  | float | 2000–16000 Hz  | 6000    | Noise highpass cutoff               |
| `shimmer` | float | 0.0–1.0        | 0.2     | Partial frequency modulation depth  |

## Acceptance criteria

- [ ] Module registered as `Cymbal` in `default_registry()`.
- [ ] Produces a long, washy metallic sound on trigger.
- [ ] `decay` supports long tails (up to 8s) for crash-like sounds.
- [ ] `shimmer` adds audible animation to the metallic spectrum.
- [ ] `pitch` and `tone` behave consistently with hi-hat modules.
- [ ] Doc comment follows module documentation standard.
- [ ] Unit tests: trigger response, long decay behaviour, shimmer
      modulation presence.
- [ ] `cargo clippy` clean, no `unwrap()`/`expect()`.

## Notes

Depends on: 0327 (MetallicTone, DecayEnvelope), 0331 (shares metallic
tone engine with hi-hats — reuse the same `MetallicTone` primitive).
Epic: E061
