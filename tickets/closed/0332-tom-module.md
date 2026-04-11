---
id: "0332"
title: Tom module
priority: medium
created: 2026-04-12
---

## Summary

Implement a self-contained tom module. Toms share the kick's basic
architecture (sine oscillator + pitch sweep + amplitude decay) but with a
higher pitch range, shorter sweep, and a subtle noise layer for attack
texture. A single `Tom` module with a wide pitch range covers low, mid,
and high toms.

## Design

Signal path:
1. Sine oscillator with `PitchSweep` (shorter and shallower than kick).
2. Mixed with a small amount of filtered noise for attack texture.
3. Shaped by `DecayEnvelope`.

### Inputs

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

### Outputs

| Port  | Kind | Description |
| ----- | ---- | ----------- |
| `out` | mono | Tom signal  |

### Parameters

| Name         | Type  | Range         | Default | Description                       |
| ------------ | ----- | ------------- | ------- | --------------------------------- |
| `pitch`      | float | 40–500 Hz     | 120     | Base pitch                        |
| `sweep`      | float | 0–2000 Hz     | 400     | Pitch sweep start offset          |
| `sweep_time` | float | 0.001–0.3 s   | 0.03    | Pitch sweep duration              |
| `decay`      | float | 0.05–2.0 s    | 0.3     | Amplitude decay time              |
| `noise`      | float | 0.0–1.0       | 0.15    | Noise layer amount                |

## Acceptance criteria

- [ ] Module registered as `Tom` in `default_registry()`.
- [ ] Produces pitched tom sound on trigger.
- [ ] `pitch` parameter covers low to high tom range.
- [ ] Pitch sweep is audible on attack.
- [ ] `noise` parameter adds attack texture without dominating.
- [ ] Doc comment follows module documentation standard.
- [ ] Unit tests: trigger response, pitch tracking, decay behaviour.
- [ ] `cargo clippy` clean, no `unwrap()`/`expect()`.

## Notes

Depends on: 0327 (DecayEnvelope, PitchSweep), 0328 (shares kick
architecture — consider extracting common pitched-drum helper if
warranted, but don't force it if the code is simple enough to duplicate).
Epic: E061
